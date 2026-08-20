import {
  IconAlertCircle,
  IconCheck,
  IconCloudDownload,
  IconFolderSearch,
  IconLink,
  IconLinkOff,
  IconLoader2,
  IconLogin2,
  IconLogout,
  IconPlayerPlay,
  IconRefresh,
  IconSearch,
  IconX,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { formatBytes, formatDate, formatRelativeDate } from "@/lib/format";
import { storeLabel } from "@/lib/stores";
import { api, getErrorMessage } from "@/lib/tauri";
import type {
  ExternalGame,
  ExternalGameRequest,
  ExternalMatchSource,
  ExternalStoreAccount,
  ExternalStoreId,
  ExternalStoreScanReport,
  StoreDetection,
} from "@/lib/types";
import "./stores-panel.css";

const DETECTION_KEY = ["external-stores", "detection"] as const;
/** Recuentos del arranque: de aquí salen los ámbitos de la barra lateral. */
const BOOTSTRAP_KEY = ["bootstrap"] as const;
const ACCOUNTS_KEY = ["external-stores", "accounts"] as const;
const GAMES_KEY = ["external-stores", "games"] as const;

/** Cuántos juegos se piden por página. La lista de Ajustes no es la biblioteca. */
const PAGE_LIMIT = 60;

const MATCH_LABELS: Record<ExternalMatchSource, string> = {
  none: "Sin emparejar",
  automatic: "Emparejado automático",
  manual: "Emparejado por ti",
};

function scanSummary(report: ExternalStoreScanReport): string {
  if (report.status === "unavailable") {
    return report.errorMessage ?? "No se encontró ese cliente en este equipo.";
  }
  if (report.status === "failed") {
    return report.errorMessage ?? "No se pudo leer la biblioteca local de esa tienda.";
  }
  const skipped = report.skipped > 0 ? `, ${report.skipped} entradas descartadas` : "";
  const expired = report.expired > 0 ? `, ${report.expired} ya no están instalados` : "";
  return `${report.discovered} juegos leídos, ${report.matched} emparejados con Steam${skipped}${expired}.`;
}

function scansSummary(reports: ExternalStoreScanReport[]): string {
  return reports.map((report) => `${storeLabel(report.store)}: ${scanSummary(report)}`).join(" ");
}

/**
 * Códigos con los que se dice «aquí falta una sesión». Es un estado distinto de
 * «no tienes el cliente» y se resuelve de otra forma, así que la tarjeta no
 * puede mezclarlos. Los dos primeros los emite el escáner de disco sobre los
 * clientes de terceros; los dos últimos, la sincronización de cuenta.
 */
const NOT_SIGNED_IN_CODES = new Set([
  "epic_not_signed_in",
  "gog_not_signed_in",
  "external_store_not_signed_in",
  "external_store_session_expired",
]);

// ---------------------------------------------------------------------------
// Sesión de cuenta
// ---------------------------------------------------------------------------

/**
 * Contrato de las órdenes de sesión de Epic y GOG.
 *
 * Los tipos viven en este fichero por el mismo motivo que los de itch.io:
 * `src/lib/types.ts` describe el contrato ya publicado y lo están tocando otros
 * trabajos en paralelo. Cuando el frente se estabilice se mudan allí sin
 * cambiar nada más.
 *
 * Aquí **no hay ningún campo secreto**, y no es casualidad: Rust nunca envía el
 * testigo ni el identificador de cuenta. Lo único que viaja sobre dónde está
 * guardado es su nombre en el llavero, que es justo lo que hace falta para
 * poder comprobarlo desde fuera.
 */
interface StoreSession {
  store: ExternalStoreId;
  displayName: string;
  signedIn: boolean;
  accountName: string | null;
  expiresAt: string | null;
  /** El testigo caduca pronto: la próxima sincronización lo renovará sola. */
  needsRefresh: boolean;
  /** Ya no se puede renovar: hay que volver a iniciar sesión. */
  refreshExpired: boolean;
  keychainService: string;
  keychainAccount: string;
  accountSessionsUrl: string | null;
  /**
   * Si el inicio de sesión puede terminarse dentro de la ventana de Vindexa.
   * Rust lo deriva de la allowlist del navegador integrado, así que la interfaz
   * no tiene que saber qué tiendas están listas: se lo preguntan.
   */
  supportsInAppLogin: boolean;
  /**
   * El llavero no dejó leer la entrada: permiso denegado, llavero bloqueado o
   * fallo del sistema.
   *
   * No es lo mismo que no tener sesión. Enseñarlo como «sin sesión iniciada»
   * invitaba a volver a entrar, que no es lo que arregla un permiso.
   */
  unreadable: boolean;
}

interface StoreLoginPrompt {
  store: ExternalStoreId;
  displayName: string;
  url: string;
  instructions: string;
  fieldLabel: string;
}

interface StoreSignOutReport {
  store: ExternalStoreId;
  displayName: string;
  tokenRemoved: boolean;
  remotelyRevoked: boolean;
  keychainEmpty: boolean;
  accountSessionsUrl: string | null;
}

const SESSIONS_KEY = ["external-stores", "sessions"] as const;

/**
 * Cuenta el cierre de sesión con hechos comprobables, no con un «listo».
 *
 * Que el testigo se haya borrado del llavero y que la tienda haya aceptado
 * revocarlo son dos cosas distintas, y sólo una de las dos depende de Vindexa.
 * Decirlo por separado es lo que permite confiar en la respuesta.
 */
function signOutSummary(report: StoreSignOutReport): string {
  if (!report.tokenRemoved) {
    return `No había ninguna sesión guardada de ${report.displayName}.`;
  }
  const local = report.keychainEmpty
    ? "El testigo se ha borrado del llavero de macOS y se ha comprobado que ya no queda nada."
    : "El testigo se ha borrado, pero la comprobación posterior no ha podido confirmarlo: revísalo en Acceso a Llaveros.";
  const remote = report.remotelyRevoked
    ? `${report.displayName} ha confirmado además que lo ha invalidado en su lado.`
    : `Vindexa no ha podido invalidarlo también en ${report.displayName}; puedes hacerlo desde los ajustes de tu cuenta.`;
  return `${local} ${remote}`;
}

/**
 * Resultado del último escaneo, dicho de forma que **nunca** contradiga a la
 * detección.
 *
 * Antes esta celda decía «Cliente no encontrado» ante cualquier estado
 * `unavailable`, incluso cuando la cabecera acababa de decir «Detectada en este
 * equipo». Las dos frases venían de sitios distintos y se desmentían entre sí.
 */
function scanResultLabel(detection: StoreDetection, account?: ExternalStoreAccount): string {
  const status = account?.lastScanStatus ?? null;
  if (status === null) return "Sin datos todavía";
  if (status === "failed") return "No se pudo leer";
  if (status === "success") {
    return (account?.gameCount ?? 0) > 0 ? "Leído correctamente" : "Leído: sin juegos";
  }
  if (NOT_SIGNED_IN_CODES.has(account?.lastScanErrorCode ?? "")) return "Sin sesión iniciada";
  return detection.detected ? "Sin biblioteca que leer" : "Cliente no encontrado";
}

/**
 * Qué hacer a continuación, cuando hay algo que hacer. Devuelve `undefined`
 * cuando la tienda ya está leída: no se rellena la tarjeta con consejos vacíos.
 *
 * El consejo depende de la sesión, no del disco. Desde que Vindexa inicia
 * sesión por su cuenta, «no tienes el cliente instalado» dejó de ser un
 * problema que resolver: la biblioteca completa llega por la cuenta, y los
 * manifiestos locales sólo añaden qué está descargado.
 */
function nextStep(
  detection: StoreDetection,
  account?: ExternalStoreAccount,
  session?: StoreSession,
): string | undefined {
  // Un permiso denegado no se arregla volviendo a entrar: el consejo tiene que
  // llevar a donde está el problema, que es el llavero del sistema.
  if (session?.unreadable) {
    return `El llavero de macOS no dejó leer la sesión de ${detection.displayName}. Puede que se denegara el permiso o que el llavero esté bloqueado: ábrelo en Acceso a Llaveros, busca «${session.keychainAccount}» dentro de «${session.keychainService}» y permite el acceso. Si prefieres empezar de cero, cierra sesión y vuelve a iniciarla.`;
  }
  if (!session?.signedIn) {
    return `Inicia sesión en ${detection.displayName} para traer tu biblioteca completa. No hace falta tener su cliente instalado.`;
  }
  if (session.refreshExpired) {
    return `La sesión de ${detection.displayName} ha caducado. Vuelve a iniciar sesión para seguir sincronizando.`;
  }
  const status = account?.lastScanStatus ?? null;
  if ((account?.gameCount ?? 0) > 0) return undefined;
  if (status === "success") {
    return `Se leyó tu cuenta de ${detection.displayName} y no había ningún juego. Si tu biblioteca no está vacía, vuelve a sincronizar.`;
  }
  return `Sincroniza tu biblioteca de ${detection.displayName} para traerla a Vindexa.`;
}

/**
 * GOG no publica un esquema para arrancar un juego: `goggalaxy://` sólo abre su
 * ficha dentro de Galaxy. Sin un ejecutable validado en disco, el botón no puede
 * prometer que va a jugar, así que dice lo que de verdad hace.
 */
function opensStorePage(game: ExternalGame): boolean {
  return game.store === "gog" && game.launchTarget === null;
}

function launchHint(game: ExternalGame): string {
  return opensStorePage(game)
    ? "GOG no expone un lanzamiento directo y aquí no hay ningún ejecutable validado: se abrirá la ficha del juego dentro de Galaxy."
    : "Vindexa entrega la orden al cliente oficial de la tienda y no conoce el resultado.";
}

export function StoresPanel() {
  const queryClient = useQueryClient();
  const [notice, setNotice] = useState<{ kind: "success" | "error"; message: string }>();
  const [storeFilter, setStoreFilter] = useState<ExternalStoreId | "all">("all");
  const [unmatchedOnly, setUnmatchedOnly] = useState(false);
  const [search, setSearch] = useState("");
  const [pickerFor, setPickerFor] = useState<{ store: ExternalStoreId; externalId: string }>();

  const detection = useQuery({
    queryKey: DETECTION_KEY,
    queryFn: () => api.detectExternalStores(),
  });
  const accounts = useQuery({
    queryKey: ACCOUNTS_KEY,
    queryFn: () => api.listExternalStoreAccounts(),
  });
  const sessions = useQuery({
    queryKey: SESSIONS_KEY,
    queryFn: () => invoke<StoreSession[]>("list_external_store_sessions"),
  });
  const gamesRequest: ExternalGameRequest = {
    ...(storeFilter === "all" ? {} : { store: storeFilter }),
    ...(search.trim() ? { query: search.trim() } : {}),
    ...(unmatchedOnly ? { matched: false } : {}),
    sort: "alphabetical",
    limit: PAGE_LIMIT,
  };
  const games = useQuery({
    queryKey: [...GAMES_KEY, storeFilter, search.trim(), unmatchedOnly],
    queryFn: () => api.listExternalGames(gamesRequest),
  });

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: DETECTION_KEY }),
      queryClient.invalidateQueries({ queryKey: ACCOUNTS_KEY }),
      queryClient.invalidateQueries({ queryKey: SESSIONS_KEY }),
      queryClient.invalidateQueries({ queryKey: GAMES_KEY }),
      // El ámbito de cada tienda en la barra lateral sale de los recuentos del
      // arranque. Sin invalidarlos, sincronizar una tienda no la hacía
      // aparecer: había que cerrar y volver a abrir la aplicación, y mientras
      // tanto parecía que la sincronización no había traído nada.
      queryClient.invalidateQueries({ queryKey: BOOTSTRAP_KEY }),
    ]);
  };

  const syncOne = useMutation({
    mutationFn: (store: ExternalStoreId) =>
      invoke<ExternalStoreScanReport>("sync_external_store_library", { store }),
    onSuccess: (report) =>
      setNotice({
        kind: report.status === "success" ? "success" : "error",
        message: `${storeLabel(report.store)}: ${scanSummary(report)}`,
      }),
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
    onSettled: () => void refresh(),
  });
  const signOut = useMutation({
    mutationFn: (store: ExternalStoreId) =>
      invoke<StoreSignOutReport>("sign_out_external_store", { store }),
    onSuccess: (report) => setNotice({ kind: "success", message: signOutSummary(report) }),
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
    onSettled: () => void refresh(),
  });

  const scanOne = useMutation({
    mutationFn: (store: ExternalStoreId) => api.scanExternalStore(store),
    onSuccess: (report) =>
      setNotice({
        kind: report.status === "success" ? "success" : "error",
        message: `${storeLabel(report.store)}: ${scanSummary(report)}`,
      }),
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
    onSettled: () => void refresh(),
  });
  const scanAll = useMutation({
    mutationFn: () => api.scanExternalStores(),
    onSuccess: (reports) => setNotice({ kind: "success", message: scansSummary(reports) }),
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
    onSettled: () => void refresh(),
  });
  const toggleLink = useMutation({
    mutationFn: async ({ store, linked }: { store: ExternalStoreId; linked: boolean }) => {
      if (linked) await api.unlinkExternalStore(store);
      else await api.linkExternalStore(store);
    },
    onSuccess: () => setNotice({ kind: "success", message: "Vinculación actualizada." }),
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
    onSettled: () => void refresh(),
  });
  const setMatch = useMutation({
    mutationFn: ({
      store,
      externalId,
      appId,
    }: {
      store: ExternalStoreId;
      externalId: string;
      appId: number | null;
    }) => api.setExternalGameMatch(store, externalId, appId),
    onSuccess: (game) =>
      setNotice({
        kind: "success",
        message: game.matchedAppId
          ? `«${game.title}» quedó emparejado con «${game.matchedTitle ?? game.matchedAppId}».`
          : `«${game.title}» quedó marcado como juego distinto a los de tu biblioteca de Steam.`,
      }),
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
    onSettled: () => {
      setPickerFor(undefined);
      void refresh();
    },
  });
  const clearMatch = useMutation({
    mutationFn: ({ store, externalId }: { store: ExternalStoreId; externalId: string }) =>
      api.clearExternalGameMatch(store, externalId),
    onSuccess: () =>
      setNotice({ kind: "success", message: "El emparejado vuelve a decidirlo Vindexa." }),
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
    onSettled: () => void refresh(),
  });
  const launch = useMutation({
    mutationFn: ({ store, externalId }: { store: ExternalStoreId; externalId: string }) =>
      api.launchExternalGame(store, externalId),
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });
  const rematch = useMutation({
    mutationFn: () => api.rematchExternalStores(),
    onSuccess: (changed) =>
      setNotice({
        kind: "success",
        message:
          changed === 0
            ? "Ningún emparejado automático ha cambiado con tu biblioteca actual de Steam."
            : `${changed} emparejados automáticos actualizados. Tus correcciones manuales siguen intactas.`,
      }),
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
    onSettled: () => void refresh(),
  });

  const busy =
    scanOne.isPending ||
    scanAll.isPending ||
    toggleLink.isPending ||
    rematch.isPending ||
    syncOne.isPending ||
    signOut.isPending;
  const detections = detection.data ?? [];
  const nothingDetected = detections.length > 0 && detections.every((entry) => !entry.detected);

  return (
    <div className="settings-section">
      <div className="settings-heading">
        <h3>Tiendas externas</h3>
        <p>
          Inicia sesión en Epic Games Store y en GOG desde la propia Vindexa y tendrás tu biblioteca
          completa aquí, no sólo lo que tengas instalado. Si además tienes sus clientes en este
          equipo, Vindexa lee sus manifiestos para saber qué está descargado.
        </p>
      </div>

      <div className="stores-explainer">
        <h4>Qué guarda Vindexa, dónde, y cómo se revoca</h4>
        <p>
          El inicio de sesión ocurre en <strong>tu navegador</strong>, en la página de la propia
          tienda: Vindexa nunca ve tu contraseña ni tu doble factor. Lo que recibe al terminar es un
          código de un solo uso que canjea por un <strong>testigo de acceso</strong>.
        </p>
        <p>
          Ese testigo se guarda en el <strong>llavero de macOS</strong> y en ningún otro sitio: ni
          en la base de datos, ni en un fichero de configuración, ni en un registro, ni en esta
          pantalla. Cada tarjeta te dice el nombre exacto con el que puedes buscarlo en Acceso a
          Llaveros y comprobarlo por tu cuenta.
        </p>
        <p>
          Al cerrar sesión el testigo se borra del llavero y Vindexa te confirma que ya no queda
          nada. Cada tarjeta enlaza además a la página de tu cuenta en la tienda, por si prefieres
          revocar la sesión también desde allí.
        </p>
      </div>

      {notice && (
        <div
          className="inline-notice"
          data-kind={notice.kind}
          role={notice.kind === "error" ? "alert" : "status"}
        >
          {notice.kind === "success" ? <IconCheck /> : <IconAlertCircle />}
          <span>{notice.message}</span>
        </div>
      )}

      <div className="button-row">
        <Button size="sm" onClick={() => scanAll.mutate()} disabled={busy}>
          {scanAll.isPending ? <IconLoader2 className="is-spinning" /> : <IconRefresh />} Escanear
          todas las tiendas
        </Button>
        <Button size="sm" variant="outline" onClick={() => rematch.mutate()} disabled={busy}>
          {rematch.isPending ? <IconLoader2 className="is-spinning" /> : <IconLink />} Reintentar
          emparejado con Steam
        </Button>
      </div>

      {detection.isPending ? (
        <p className="settings-hint">Comprobando qué tiendas hay en este equipo…</p>
      ) : detection.isError ? (
        <div className="inline-notice" data-kind="error" role="alert">
          <IconAlertCircle />
          <span>{getErrorMessage(detection.error)}</span>
        </div>
      ) : (
        <div className="stores-grid">
          {detections.map((entry) => (
            <StoreCard
              key={entry.store}
              detection={entry}
              account={accounts.data?.find((item) => item.store === entry.store)}
              session={sessions.data?.find((item) => item.store === entry.store)}
              busy={busy}
              onScan={() => scanOne.mutate(entry.store)}
              onSync={() => syncOne.mutate(entry.store)}
              onSignOut={() => signOut.mutate(entry.store)}
              onSignedIn={() => void refresh()}
              onToggleLink={(linked) => toggleLink.mutate({ store: entry.store, linked })}
            />
          ))}
        </div>
      )}

      {nothingDetected && (
        <p className="settings-hint">
          No hay ningún cliente de Epic ni de GOG instalado en este equipo. Eso ya no te impide ver
          tu biblioteca: inicia sesión en cada tarjeta y llegará entera desde tu cuenta. Los
          clientes locales sólo aportan un dato que la cuenta no conoce —qué tienes descargado—, y
          cada tarjeta enumera las rutas exactas donde se buscan.
        </p>
      )}

      <div className="settings-divider" />

      <ItchPanel />

      <div className="settings-divider" />

      <div className="settings-heading">
        <h3>Juegos detectados</h3>
        <p>
          Lo que hay hoy en los ficheros de esas tiendas, con su estado de emparejado contra tu
          biblioteca de Steam. Un juego que poseas pero no tengas instalado también aparece aquí,
          sin ruta de instalación. Un emparejado que corrijas a mano sobrevive a todos los escaneos
          posteriores.
        </p>
      </div>

      <div className="stores-filters">
        <div className="stores-search">
          <IconSearch aria-hidden="true" size={15} />
          <Input
            aria-label="Buscar entre los juegos externos"
            placeholder="Buscar por título"
            value={search}
            onChange={(event) => setSearch(event.currentTarget.value)}
          />
        </div>
        <fieldset className="stores-chips">
          <legend className="sr-only">Filtrar por tienda</legend>
          {(["all", "epic", "gog"] as const).map((value) => (
            <button
              key={value}
              type="button"
              data-active={storeFilter === value}
              aria-pressed={storeFilter === value}
              onClick={() => setStoreFilter(value)}
            >
              {value === "all" ? "Todas" : storeLabel(value)}
            </button>
          ))}
        </fieldset>
        <div className="stores-toggle">
          <Switch
            id="stores-unmatched-only"
            checked={unmatchedOnly}
            onCheckedChange={(checked) => setUnmatchedOnly(checked)}
          />
          <label htmlFor="stores-unmatched-only">Solo sin emparejar</label>
        </div>
      </div>

      {games.isPending ? (
        <p className="settings-hint">Cargando los juegos detectados…</p>
      ) : games.isError ? (
        <div className="inline-notice" data-kind="error" role="alert">
          <IconAlertCircle />
          <span>{getErrorMessage(games.error)}</span>
        </div>
      ) : !games.data || games.data.items.length === 0 ? (
        <p className="settings-hint">
          {search.trim() || unmatchedOnly || storeFilter !== "all"
            ? "Ningún juego detectado cumple ese filtro."
            : "Todavía no hay ningún juego detectado. Escanea una tienda para leer sus manifiestos."}
        </p>
      ) : (
        <>
          <ul className="stores-games">
            {games.data.items.map((game) => (
              <ExternalGameRow
                key={`${game.store}:${game.externalId}`}
                game={game}
                pickerOpen={
                  pickerFor?.store === game.store && pickerFor.externalId === game.externalId
                }
                busy={setMatch.isPending || clearMatch.isPending}
                onLaunch={() => launch.mutate({ store: game.store, externalId: game.externalId })}
                onOpenPicker={() =>
                  setPickerFor({ store: game.store, externalId: game.externalId })
                }
                onClosePicker={() => setPickerFor(undefined)}
                onPick={(appId) =>
                  setMatch.mutate({ store: game.store, externalId: game.externalId, appId })
                }
                onClearMatch={() =>
                  clearMatch.mutate({ store: game.store, externalId: game.externalId })
                }
              />
            ))}
          </ul>
          {games.data.total > games.data.items.length && (
            <p className="settings-hint">
              Se muestran {games.data.items.length} de {games.data.total} juegos detectados. Afina
              la búsqueda para ver el resto.
            </p>
          )}
        </>
      )}
    </div>
  );
}

/**
 * Iniciar sesión, mantenerla y cerrarla, dentro de la tarjeta de su tienda.
 *
 * El bloque enseña siempre dónde vive el testigo, también cuando no hay
 * ninguno. Es deliberado: quien acaba de cerrar sesión necesita saber qué
 * buscar en Acceso a Llaveros para comprobar que no queda nada, y esconder el
 * dato justo entonces sería esconderlo cuando más falta hace.
 */
function StoreSessionBlock({
  detection,
  session,
  busy,
  onSync,
  onSignOut,
  onSignedIn,
}: {
  detection: StoreDetection;
  session: StoreSession | undefined;
  busy: boolean;
  onSync: () => void;
  onSignOut: () => void;
  onSignedIn: () => void;
}) {
  const [prompt, setPrompt] = useState<StoreLoginPrompt>();
  const [code, setCode] = useState("");
  const [error, setError] = useState<string>();

  /**
   * El camino normal: se abre la tienda en la ventana de Vindexa, la persona se
   * identifica allí y el código lo recoge Rust de la página de retorno. No hay
   * nada que copiar.
   */
  const signIn = useMutation({
    mutationFn: () => invoke<StoreSession>("sign_in_external_store", { store: detection.store }),
    onSuccess: () => {
      setError(undefined);
      setPrompt(undefined);
      onSignedIn();
    },
    onError: (cause) => setError(getErrorMessage(cause)),
  });

  const begin = useMutation({
    mutationFn: () =>
      invoke<StoreLoginPrompt>("begin_external_store_login", { store: detection.store }),
    onSuccess: (result) => {
      setPrompt(result);
      setError(undefined);
    },
    onError: (cause) => setError(getErrorMessage(cause)),
  });

  const complete = useMutation({
    mutationFn: (value: string) =>
      invoke<StoreSession>("complete_external_store_login", {
        store: detection.store,
        code: value,
      }),
    onSuccess: () => {
      // El código se borra del formulario en cuanto deja de hacer falta: es de
      // un solo uso, pero no tiene por qué quedarse a la vista.
      setCode("");
      setPrompt(undefined);
      setError(undefined);
      onSignedIn();
    },
    onError: (cause) => setError(getErrorMessage(cause)),
  });

  const signedIn = session?.signedIn === true;
  // Sin sesión todavía no hay retrato: se asume que sí, y Rust corrige.
  const inApp = session?.supportsInAppLogin !== false;
  const working = busy || signIn.isPending || begin.isPending || complete.isPending;

  return (
    <div className="store-session">
      {signedIn ? (
        <>
          <dl className="store-session__facts">
            <div>
              <dt>Cuenta</dt>
              <dd>{session?.accountName ?? "Sesión iniciada"}</dd>
            </div>
            <div>
              <dt>El testigo caduca</dt>
              <dd title={formatDate(session?.expiresAt ?? undefined)}>
                {session?.refreshExpired
                  ? "Caducada: vuelve a iniciar sesión"
                  : session?.expiresAt
                    ? `${formatRelativeDate(session.expiresAt)} · se renueva solo`
                    : "Se renueva solo"}
              </dd>
            </div>
          </dl>
          <div className="button-row">
            <Button size="sm" onClick={onSync} disabled={working}>
              <IconCloudDownload /> Sincronizar biblioteca
            </Button>
            <Button size="sm" variant="ghost" onClick={onSignOut} disabled={working}>
              <IconLogout /> Cerrar sesión
            </Button>
          </div>
        </>
      ) : prompt ? (
        <div className="store-session__login">
          <p>{prompt.instructions}</p>
          <label htmlFor={`store-code-${detection.store}`}>{prompt.fieldLabel}</label>
          <div className="store-session__field">
            <Input
              id={`store-code-${detection.store}`}
              autoFocus
              value={code}
              spellCheck={false}
              autoComplete="off"
              onChange={(event) => setCode(event.currentTarget.value)}
              placeholder="Pega aquí lo que te ha dado la tienda"
            />
            <Button
              size="sm"
              onClick={() => complete.mutate(code)}
              disabled={working || code.trim().length === 0}
            >
              {complete.isPending ? <IconLoader2 className="is-spinning" /> : <IconCheck />}{" "}
              Conectar
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                setPrompt(undefined);
                setCode("");
                setError(undefined);
              }}
              disabled={complete.isPending}
              aria-label="Cancelar el inicio de sesión"
            >
              <IconX />
            </Button>
          </div>
          <p className="store-session__reopen">
            ¿No se ha abierto la página?{" "}
            <button type="button" onClick={() => begin.mutate()} disabled={working}>
              Vuelve a abrirla
            </button>
            .
          </p>
        </div>
      ) : (
        <div className="store-session__start">
          <div className="button-row">
            <Button
              size="sm"
              onClick={() => (inApp ? signIn.mutate() : begin.mutate())}
              disabled={working}
            >
              {signIn.isPending || begin.isPending ? (
                <IconLoader2 className="is-spinning" />
              ) : (
                <IconLogin2 />
              )}{" "}
              Iniciar sesión en {detection.displayName}
            </Button>
          </div>
          {signIn.isPending ? (
            <p className="store-session__waiting" role="status">
              Se ha abierto {detection.displayName} en una ventana de Vindexa. Identifícate ahí y
              esto terminará solo. Si cambias de idea, cierra esa ventana.
            </p>
          ) : inApp ? (
            <p className="store-session__reopen">
              ¿Prefieres identificarte en tu navegador de siempre?{" "}
              <button type="button" onClick={() => begin.mutate()} disabled={working}>
                Hazlo a mano
              </button>
              .
            </p>
          ) : (
            <p className="store-session__reopen">
              {detection.displayName} se identifica en tu navegador y te pedirá pegar aquí lo que te
              devuelva.
            </p>
          )}
        </div>
      )}

      {error && (
        <p className="store-session__error" role="alert">
          {error}
        </p>
      )}

      <p className="store-session__custody">
        {signedIn
          ? "El testigo está guardado en el llavero de macOS, en «"
          : "Si inicias sesión, el testigo se guardará en el llavero de macOS, en «"}
        <code>{session?.keychainService ?? "io.vindexa.desktop"}</code> ·{" "}
        <code>{session?.keychainAccount ?? `${detection.store}-store-session`}</code>
        {"». Vindexa no lo muestra nunca ni lo escribe en la base de datos."}
        {session?.accountSessionsUrl && (
          <>
            {" "}
            También puedes revocarlo desde{" "}
            <a href={session.accountSessionsUrl} target="_blank" rel="noreferrer noopener">
              los ajustes de tu cuenta
            </a>
            .
          </>
        )}
      </p>
    </div>
  );
}

function StoreCard({
  detection,
  account,
  session,
  busy,
  onScan,
  onSync,
  onSignOut,
  onSignedIn,
  onToggleLink,
}: {
  detection: StoreDetection;
  account: ExternalStoreAccount | undefined;
  session: StoreSession | undefined;
  busy: boolean;
  onScan: () => void;
  onSync: () => void;
  onSignOut: () => void;
  onSignedIn: () => void;
  onToggleLink: (linked: boolean) => void;
}) {
  const [showPaths, setShowPaths] = useState(false);
  const status = account?.lastScanStatus ?? null;
  const advice = nextStep(detection, account, session);
  const signedIn = session?.signedIn === true;
  return (
    <article
      className="store-card"
      aria-label={detection.displayName}
      data-detected={detection.detected}
      data-signed-in={signedIn}
    >
      <header className="store-card__head">
        <div>
          <strong>{detection.displayName}</strong>
          <span data-state={signedIn ? "detected" : session?.unreadable ? "warning" : "absent"}>
            {signedIn
              ? session?.accountName
                ? `Sesión iniciada como ${session.accountName}`
                : "Sesión iniciada"
              : session?.unreadable
                ? "No se pudo leer la sesión guardada"
                : "Sin sesión iniciada"}
          </span>
        </div>
        <span className="store-card__count">
          {account?.gameCount ?? 0} {account?.gameCount === 1 ? "juego" : "juegos"}
        </span>
      </header>

      <dl className="store-card__facts">
        <div>
          <dt>Última lectura</dt>
          <dd title={formatDate(account?.lastScanAt ?? undefined)}>
            {account?.lastScanAt ? formatRelativeDate(account.lastScanAt) : "Nunca"}
          </dd>
        </div>
        <div>
          <dt>Resultado</dt>
          <dd data-status={status ?? "none"}>{scanResultLabel(detection, account)}</dd>
        </div>
        <div>
          <dt>Cliente en este equipo</dt>
          <dd data-status={detection.detected ? "success" : "none"}>
            {detection.detected ? "Detectado" : "No hace falta"}
          </dd>
        </div>
      </dl>

      <StoreSessionBlock
        detection={detection}
        session={session}
        busy={busy}
        onSync={onSync}
        onSignOut={onSignOut}
        onSignedIn={onSignedIn}
      />

      {account?.lastScanErrorMessage && (
        <p className="store-card__error" role="status">
          {account.lastScanErrorMessage}
        </p>
      )}

      {advice && <p className="store-card__advice">{advice}</p>}

      {detection.detected && detection.detectedPaths.length > 0 && (
        <div className="store-card__paths">
          <span>Orígenes encontrados</span>
          <ul>
            {detection.detectedPaths.map((path) => (
              <li key={path}>
                <code>{path}</code>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="button-row">
        <Button size="sm" variant="outline" onClick={onScan} disabled={busy}>
          <IconRefresh /> Escanear
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => onToggleLink(account?.linked ?? false)}
          disabled={busy}
        >
          {account?.linked ? <IconLinkOff /> : <IconLink />}
          {account?.linked ? "Desvincular" : "Vincular"}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          aria-expanded={showPaths}
          onClick={() => setShowPaths((value) => !value)}
        >
          <IconFolderSearch /> {showPaths ? "Ocultar dónde se busca" : "Ver dónde se busca"}
        </Button>
      </div>

      {showPaths && (
        <div className="store-card__paths" data-variant="searched">
          <span>Rutas que se comprueban en este equipo</span>
          <ul>
            {detection.searchedPaths.map((path) => (
              <li key={path}>
                <code>{path}</code>
              </li>
            ))}
          </ul>
        </div>
      )}
    </article>
  );
}

function ExternalGameRow({
  game,
  pickerOpen,
  busy,
  onLaunch,
  onOpenPicker,
  onClosePicker,
  onPick,
  onClearMatch,
}: {
  game: ExternalGame;
  pickerOpen: boolean;
  busy: boolean;
  onLaunch: () => void;
  onOpenPicker: () => void;
  onClosePicker: () => void;
  onPick: (appId: number | null) => void;
  onClearMatch: () => void;
}) {
  return (
    <li className="stores-game">
      <div className="stores-game__identity">
        <strong>{game.title}</strong>
        <span>
          {storeLabel(game.store)}
          {game.installed ? " · Instalado" : " · No instalado"}
          {game.sizeOnDisk ? ` · ${formatBytes(game.sizeOnDisk)}` : ""}
        </span>
      </div>
      <div className="stores-game__match" data-source={game.matchSource}>
        <span>{MATCH_LABELS[game.matchSource]}</span>
        {game.matchedTitle && <strong>{game.matchedTitle}</strong>}
      </div>
      <div className="button-row">
        {game.installed && (
          <Button size="sm" variant="outline" onClick={onLaunch} title={launchHint(game)}>
            <IconPlayerPlay /> {opensStorePage(game) ? "Abrir en GOG Galaxy" : "Jugar"}
          </Button>
        )}
        <Button size="sm" variant="ghost" onClick={onOpenPicker} disabled={busy}>
          Emparejar
        </Button>
        {game.matchedAppId !== null && (
          <Button size="sm" variant="ghost" onClick={() => onPick(null)} disabled={busy}>
            Desemparejar
          </Button>
        )}
        {game.matchSource === "manual" && (
          <Button size="sm" variant="ghost" onClick={onClearMatch} disabled={busy}>
            Volver al automático
          </Button>
        )}
      </div>
      {pickerOpen && <SteamGamePicker onPick={onPick} onClose={onClosePicker} busy={busy} />}
    </li>
  );
}

function SteamGamePicker({
  onPick,
  onClose,
  busy,
}: {
  onPick: (appId: number) => void;
  onClose: () => void;
  busy: boolean;
}) {
  const [query, setQuery] = useState("");
  const trimmed = query.trim();
  const results = useQuery({
    queryKey: ["external-stores", "steam-picker", trimmed],
    queryFn: () => api.listGames({ query: trimmed, limit: 8, sort: "alphabetical" }),
    enabled: trimmed.length >= 2,
  });
  return (
    <div className="stores-picker">
      <div className="stores-search">
        <IconSearch aria-hidden="true" size={15} />
        <Input
          autoFocus
          aria-label="Buscar el juego de Steam con el que emparejar"
          placeholder="Escribe al menos dos letras del título en Steam"
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
        />
        <Button size="sm" variant="ghost" onClick={onClose} aria-label="Cerrar el emparejado">
          <IconX />
        </Button>
      </div>
      {trimmed.length < 2 ? (
        <p className="settings-hint">
          Vindexa no propone nada por su cuenta aquí: elige tú el juego de tu biblioteca.
        </p>
      ) : results.isPending ? (
        <p className="settings-hint">Buscando en tu biblioteca de Steam…</p>
      ) : results.data && results.data.items.length > 0 ? (
        <ul className="stores-picker__results">
          {results.data.items.map((candidate) => (
            <li key={candidate.appId}>
              <button type="button" disabled={busy} onClick={() => onPick(candidate.appId)}>
                <span>{candidate.title}</span>
                <small>AppID {candidate.appId}</small>
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="settings-hint">Ningún juego de tu biblioteca coincide con esa búsqueda.</p>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// itch.io
// ---------------------------------------------------------------------------

/**
 * itch.io se conecta de otra forma que Epic y GOG, y por un buen motivo: es la
 * única de las tres que publica una API oficial con claves que genera la propia
 * persona usuaria. Aquí no se suplanta a ningún cliente ni se guarda una
 * contraseña; se guarda una clave revocable, y en el llavero del sistema.
 *
 * Los tipos viven en este fichero a propósito: `src/lib/types.ts` describe el
 * contrato ya publicado de Epic y GOG y lo está tocando otro trabajo. Cuando
 * `ExternalStoreId` admita `"itch"`, estos tipos se mudan allí sin cambiar nada
 * más.
 */
const ITCH_SESSION_KEY = ["external-stores", "itch", "session"] as const;

/** Dónde genera la persona usuaria su clave. Ahorra buscarla. */
const ITCH_API_KEYS_URL = "https://itch.io/user/settings/api-keys";

interface ItchSkipGroup {
  /** Valor tal y como lo devolvió itch.io, o el motivo interno de Vindexa. */
  reason: string;
  label: string;
  count: number;
}

interface ItchImportReport {
  account: string;
  ownedKeys: number;
  imported: number;
  added: number;
  alreadyPresent: number;
  withoutCover: number;
  skipped: number;
  skippedGroups: ItchSkipGroup[];
  matched: number;
  truncated: boolean;
}

interface ItchAccountProfile {
  username: string;
  displayName: string | null;
}

interface ItchSessionState {
  /** ¿Hay una clave guardada en el llavero? El valor **nunca** viaja. */
  hasKey: boolean;
  account: string | null;
  lastImportAt: string | null;
  lastImportStatus: "success" | "failed" | null;
  lastImportErrorMessage: string | null;
  gameCount: number;
}

/**
 * Cuenta la importación con números, no con un visto bueno.
 *
 * Un «importado ✓» esconde justo lo que hay que explicar: que itch.io vende
 * mucho más que juegos y que buena parte de lo que posees no entra en una
 * biblioteca de juegos.
 */
function itchImportSummary(report: ItchImportReport): string {
  const nuevos = report.added === 1 ? "1 es nuevo" : `${report.added} son nuevos`;
  const repetidos =
    report.alreadyPresent === 1 ? "1 ya estaba" : `${report.alreadyPresent} ya estaban`;
  return `Tu cuenta tiene ${report.ownedKeys} elementos. ${report.imported} son juegos: ${nuevos} y ${repetidos}.`;
}

function ItchPanel() {
  const queryClient = useQueryClient();
  const [key, setKey] = useState("");
  const [report, setReport] = useState<ItchImportReport>();
  const [notice, setNotice] = useState<{ kind: "success" | "error"; message: string }>();

  const session = useQuery({
    queryKey: ITCH_SESSION_KEY,
    queryFn: () => invoke<ItchSessionState>("itch_session_state"),
  });

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ITCH_SESSION_KEY });
    void queryClient.invalidateQueries({ queryKey: ACCOUNTS_KEY });
    void queryClient.invalidateQueries({ queryKey: GAMES_KEY });
    // Igual que en el panel de Epic y GOG: sin esto, itch.io no aparece en la
    // barra lateral hasta reiniciar, por mucho que la importación diga que ha
    // traído juegos.
    void queryClient.invalidateQueries({ queryKey: BOOTSTRAP_KEY });
  };

  const connect = useMutation({
    mutationFn: (value: string) => invoke<ItchAccountProfile>("save_itch_api_key", { key: value }),
    onSuccess: (profile) => {
      // La clave se borra del formulario en cuanto deja de hacer falta.
      setKey("");
      setNotice({
        kind: "success",
        message: `Sesión iniciada en itch.io como ${profile.displayName ?? profile.username}.`,
      });
      refresh();
    },
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });

  const importLibrary = useMutation({
    mutationFn: () => invoke<ItchImportReport>("import_itch_library"),
    onSuccess: (result) => {
      setReport(result);
      setNotice(undefined);
      refresh();
    },
    onError: (error) => {
      setReport(undefined);
      setNotice({ kind: "error", message: getErrorMessage(error) });
    },
  });

  const signOut = useMutation({
    mutationFn: (forget: boolean) =>
      invoke<unknown>(forget ? "forget_itch_library" : "sign_out_itch"),
    onSuccess: (_result, forget) => {
      setReport(undefined);
      setNotice({
        kind: "success",
        message: forget
          ? "Se ha borrado la clave del llavero y todo lo que se importó de itch.io."
          : "Se ha borrado la clave del llavero. Los juegos ya importados siguen en tu biblioteca.",
      });
      refresh();
    },
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });

  const busy = connect.isPending || importLibrary.isPending || signOut.isPending;
  const state = session.data;
  const connected = state?.hasKey === true;

  return (
    <section className="itch-panel">
      <div className="settings-heading">
        <h3>itch.io</h3>
        <p>
          itch.io es la única de estas tiendas que publica una API oficial, así que aquí Vindexa sí
          inicia sesión: tú generas una clave desde los ajustes de tu cuenta y la revocas cuando
          quieras. No se suplanta a ningún cliente ni se te pide la contraseña.
        </p>
      </div>

      {notice && (
        <div className="inline-notice" data-kind={notice.kind}>
          {notice.kind === "success" ? <IconCheck /> : <IconAlertCircle />}
          <span>{notice.message}</span>
        </div>
      )}

      {session.isError && (
        <p className="settings-hint" data-kind="error">
          No se pudo consultar el estado de itch.io: {getErrorMessage(session.error)}
        </p>
      )}

      {connected ? (
        <div className="itch-session">
          <p className="itch-session__account">
            <IconCheck aria-hidden="true" /> Sesión iniciada
            {state?.account ? (
              <>
                {" "}
                como <strong>{state.account}</strong>
              </>
            ) : null}
            .
          </p>
          <p className="settings-hint">
            {state?.gameCount ? `${state.gameCount} juegos de itch.io en tu biblioteca. ` : ""}
            {state?.lastImportAt
              ? `Última importación ${formatRelativeDate(state.lastImportAt)}.`
              : "Todavía no has importado nada."}
          </p>
          {state?.lastImportStatus === "failed" && state.lastImportErrorMessage && (
            <p className="settings-hint" data-kind="error">
              La última importación falló: {state.lastImportErrorMessage}
            </p>
          )}

          <div className="button-row">
            <Button size="sm" onClick={() => importLibrary.mutate()} disabled={busy}>
              {importLibrary.isPending ? <IconLoader2 className="is-spinning" /> : <IconRefresh />}{" "}
              Importar mi biblioteca
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => signOut.mutate(false)}
              disabled={busy}
            >
              <IconLinkOff /> Cerrar sesión
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => signOut.mutate(true)}
              disabled={busy}
            >
              <IconX /> Cerrar sesión y borrar lo importado
            </Button>
          </div>
        </div>
      ) : (
        <form
          className="itch-connect"
          onSubmit={(event) => {
            event.preventDefault();
            if (key.trim()) {
              connect.mutate(key.trim());
            }
          }}
        >
          <label className="itch-connect__field" htmlFor="itch-api-key">
            <span>Clave de la API de itch.io</span>
            <Input
              id="itch-api-key"
              type="password"
              value={key}
              autoComplete="off"
              spellCheck={false}
              placeholder="Pega aquí tu clave"
              onChange={(event) => setKey(event.target.value)}
            />
          </label>
          <Button size="sm" type="submit" disabled={busy || !key.trim()}>
            {connect.isPending ? <IconLoader2 className="is-spinning" /> : <IconLink />} Guardar
            clave y conectar
          </Button>
          <p className="settings-hint">
            Genera una clave en{" "}
            <button
              type="button"
              className="settings-link"
              onClick={() => openUrl(ITCH_API_KEYS_URL)}
            >
              itch.io/user/settings/api-keys
            </button>
            . Puedes revocarla desde esa misma página cuando quieras, y Vindexa dejará de tener
            acceso al instante.
          </p>
        </form>
      )}

      {report && (
        <div className="itch-report">
          <p className="itch-report__headline">{itchImportSummary(report)}</p>
          <ul className="itch-report__figures">
            <li>{report.matched} emparejados con un juego de tu biblioteca de Steam.</li>
            {report.withoutCover > 0 && (
              <li>
                {report.withoutCover} entraron sin carátula porque itch.io no publica ninguna.
              </li>
            )}
            {report.truncated && (
              <li>
                Tu cuenta supera el máximo que se lee de una vez, así que quedaron elementos sin
                revisar.
              </li>
            )}
          </ul>
          {report.skipped > 0 && (
            <details className="itch-report__skipped">
              <summary>{report.skipped} elementos quedaron fuera porque no son juegos</summary>
              <ul>
                {report.skippedGroups.map((group) => (
                  <li key={group.reason}>
                    {group.label}: {group.count}
                  </li>
                ))}
              </ul>
            </details>
          )}
        </div>
      )}

      <div className="stores-explainer">
        <h4>Qué guarda Vindexa y dónde</h4>
        <p>
          <strong>La clave va al llavero de macOS</strong>, bajo el servicio{" "}
          <code>io.vindexa.desktop</code> y la cuenta <code>itch-api-key</code>. Puedes comprobarlo
          en Acceso a Llaveros. No se escribe en la base de datos, ni en un fichero de
          configuración, ni en un registro, ni vuelve nunca a esta pantalla: ningún mensaje de error
          la incluye, tampoco recortada.
        </p>
        <p>
          <strong>En la base de datos local</strong> quedan sólo el título, la carátula y el
          identificador de cada juego, junto al nombre de tu cuenta de itch.io para poder mostrarte
          con quién has entrado. Al cerrar sesión se borra la clave del llavero; los juegos ya
          importados se conservan, porque borrarlos destruiría de paso los emparejados que hayas
          corregido a mano. Si prefieres no dejar rastro, usa «Cerrar sesión y borrar lo importado».
        </p>
        <p>
          <strong>De tu cuenta sólo entran los juegos.</strong> itch.io vende también herramientas,
          paquetes de recursos, cómics, libros y bandas sonoras, y eso no es una biblioteca de
          juegos. Lo que queda fuera se cuenta y se detalla arriba en vez de desaparecer sin más.
        </p>
      </div>
    </section>
  );
}
