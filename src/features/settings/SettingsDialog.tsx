import { zodResolver } from "@hookform/resolvers/zod";
import {
  IconAlertCircle,
  IconBrandSteam,
  IconBuildingStore,
  IconCheck,
  IconDatabase,
  IconDeviceDesktop,
  IconDownload,
  IconExternalLink,
  IconEye,
  IconEyeOff,
  IconFolderOpen,
  IconKey,
  IconKeyboard,
  IconLoader2,
  IconRefresh,
  IconRobot,
  IconRoute,
  IconShieldLock,
  IconTrash,
  IconUnlink,
  IconUpload,
  IconUsersGroup,
  IconWorld,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Fragment, useEffect, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { CopyableValue } from "@/components/motion";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { AgentsPanel } from "@/features/settings/AgentsPanel";
import { describeFamilyStatus, describeFamilySync } from "@/features/settings/family-session";
import { OrganizationSettings } from "@/features/settings/OrganizationSettings";
import { StoresPanel } from "@/features/settings/StoresPanel";
import {
  type AnyShortcutAction,
  DEFAULT_SHORTCUTS,
  describeShortcutRejection,
  eventToShortcut,
  findShortcutCollision,
  LEGACY_SEARCH_SHORTCUT,
  type LocalShortcutAction,
  readLocalShortcuts,
  resolveShortcuts,
  SHORTCUT_CATALOGUE,
  SHORTCUT_SCOPE_HINTS,
  SHORTCUT_SCOPE_LABELS,
  type ShortcutScope,
  shortcutLabel,
  writeLocalShortcuts,
} from "@/features/shell/shortcuts";
import { formatBytes, formatDate, formatRelativeDate } from "@/lib/format";
import { invalidateSteamDerivedQueries } from "@/lib/steam-data-invalidation";
import { api, getErrorMessage } from "@/lib/tauri";
import type { AppBootstrap, AppPreferences, ShortcutBindings, SteamSyncResult } from "@/lib/types";

const apiKeySchema = z.object({
  apiKey: z.string().trim().min(16, "Introduce una Web API Key válida.").max(128),
});
type ApiKeyForm = z.infer<typeof apiKeySchema>;

export type SettingsSection =
  | "steam"
  | "stores"
  | "agents"
  | "organization"
  | "appearance"
  | "shortcuts"
  | "data"
  | "privacy"
  | "about";

interface SettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  bootstrap?: AppBootstrap | undefined;
  /**
   * Sección en la que se abre. Quien llega desde un atajo —«Editar estados…»
   * en el menú de un estado— ya sabe a qué venía: aterrizar en Steam y tener
   * que buscarla sería devolverle el trabajo.
   */
  initialSection?: SettingsSection | undefined;
}

export function SettingsDialog({
  open: dialogOpen,
  onOpenChange,
  bootstrap,
  initialSection,
}: SettingsDialogProps) {
  const [section, setSection] = useState<SettingsSection>(initialSection ?? "steam");
  return (
    <Dialog open={dialogOpen} onOpenChange={onOpenChange}>
      <DialogContent className="settings-dialog" showCloseButton>
        <DialogHeader className="settings-dialog__header">
          <DialogTitle>Ajustes de Vindexa</DialogTitle>
          <DialogDescription>
            Cuenta, sincronización, comportamiento y datos locales.
          </DialogDescription>
        </DialogHeader>
        <div className="settings-dialog__body">
          <nav className="settings-nav" aria-label="Apartados de ajustes">
            <SettingsNavItem
              active={section === "steam"}
              icon={IconBrandSteam}
              label="Steam"
              onClick={() => setSection("steam")}
            />
            <SettingsNavItem
              active={section === "stores"}
              icon={IconBuildingStore}
              label="Tiendas"
              onClick={() => setSection("stores")}
            />
            <SettingsNavItem
              active={section === "agents"}
              icon={IconRobot}
              label="Agentes"
              onClick={() => setSection("agents")}
            />
            <SettingsNavItem
              active={section === "organization"}
              icon={IconRoute}
              label="Organización"
              onClick={() => setSection("organization")}
            />
            <SettingsNavItem
              active={section === "appearance"}
              icon={IconDeviceDesktop}
              label="Apariencia"
              onClick={() => setSection("appearance")}
            />
            <SettingsNavItem
              active={section === "shortcuts"}
              icon={IconKeyboard}
              label="Atajos"
              onClick={() => setSection("shortcuts")}
            />
            <SettingsNavItem
              active={section === "data"}
              icon={IconDatabase}
              label="Datos y copias"
              onClick={() => setSection("data")}
            />
            <SettingsNavItem
              active={section === "privacy"}
              icon={IconShieldLock}
              label="Privacidad"
              onClick={() => setSection("privacy")}
            />
            <SettingsNavItem
              active={section === "about"}
              icon={IconAlertCircle}
              label="Acerca de"
              onClick={() => setSection("about")}
            />
          </nav>
          <div className="settings-pane">
            {section === "steam" && <SteamSettings bootstrap={bootstrap} />}
            {section === "stores" && <StoresPanel />}
            {section === "agents" && <AgentsPanel />}
            {section === "organization" && <OrganizationSettings bootstrap={bootstrap} />}
            {section === "appearance" && <AppearanceSettings bootstrap={bootstrap} />}
            {section === "shortcuts" && <ShortcutSettings bootstrap={bootstrap} />}
            {section === "data" && <DataSettings bootstrap={bootstrap} />}
            {section === "privacy" && <PrivacySettings />}
            {section === "about" && <AboutSettings bootstrap={bootstrap} />}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function SettingsNavItem({
  active,
  icon: NavIcon,
  label,
  onClick,
}: {
  active: boolean;
  icon: typeof IconBrandSteam;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      data-active={active}
      aria-current={active ? "page" : undefined}
      onClick={onClick}
    >
      <NavIcon aria-hidden="true" size={17} />
      <span>{label}</span>
    </button>
  );
}

function steamSyncSummary(result: SteamSyncResult): string {
  const family =
    result.familyMembersDetected > 0
      ? ` Steam Family: ${result.familyCatalogGamesDetected} visibles, ${result.familyGamesImported} confirmados localmente${
          result.familyMembersInaccessible > 0
            ? ` y ${result.familyMembersInaccessible} perfiles sin acceso.`
            : "."
        }`
      : "";
  return `Sincronización completada: ${result.importedGames} importados y ${result.updatedGames} actualizados.${family}`;
}

function SteamSettings({ bootstrap }: { bootstrap?: AppBootstrap | undefined }) {
  const queryClient = useQueryClient();
  const [notice, setNotice] = useState<{ kind: "success" | "error"; message: string }>();
  const [showKey, setShowKey] = useState(false);
  const account = bootstrap?.steam.account;
  const form = useForm<ApiKeyForm>({
    resolver: zodResolver(apiKeySchema),
    defaultValues: { apiKey: "" },
  });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
  const refreshSteamData = () => invalidateSteamDerivedQueries(queryClient);
  const run = <T,>(
    operation: () => Promise<T>,
    success: (result: T) => string,
    onSettled: () => Promise<void> = refresh,
  ) => ({
    mutationFn: operation,
    onSuccess: (result: T) => {
      setNotice({ kind: "success", message: success(result) });
    },
    onError: (error: unknown) => setNotice({ kind: "error", message: getErrorMessage(error) }),
    onSettled: () => void onSettled(),
  });
  const login = useMutation(
    run(api.startSteamLogin, () => "Cuenta de Steam vinculada correctamente."),
  );
  const sync = useMutation(run(api.syncSteamLibrary, steamSyncSummary, refreshSteamData));
  const localImport = useMutation(
    run(
      api.importLocalSteam,
      (result) =>
        `Se han leído ${result.importedGames} manifiestos en ${result.librariesScanned} bibliotecas.`,
      refreshSteamData,
    ),
  );
  const saveKey = useMutation({
    mutationFn: async (data: ApiKeyForm) => {
      await api.saveSteamApiKey(data.apiKey);
      if (!account) return { kind: "saved" } as const;

      try {
        const result = await api.syncSteamLibrary();
        return { kind: "synced", result } as const;
      } catch (cause) {
        return { kind: "sync-error", message: getErrorMessage(cause) } as const;
      }
    },
    onSuccess: (result) => {
      form.reset();
      setShowKey(false);
      if (result.kind === "sync-error") {
        setNotice({ kind: "error", message: result.message });
      } else if (result.kind === "synced") {
        setNotice({
          kind: "success",
          message: `Clave guardada. ${steamSyncSummary(result.result)}`,
        });
      } else {
        setNotice({
          kind: "success",
          message: "La Web API Key se guardó en el almacén seguro del sistema.",
        });
      }
    },
    onError: (cause) => setNotice({ kind: "error", message: getErrorMessage(cause) }),
    onSettled: () => void refreshSteamData(),
  });
  const verifyKey = useMutation({
    mutationFn: api.verifySavedSteamApiKey,
    onSuccess: (configured) => {
      setNotice(
        configured
          ? {
              kind: "success",
              message: "La clave guardada está disponible para sincronizar con Steam.",
            }
          : {
              kind: "error",
              message: "No se encontró una Web API Key guardada en el almacén seguro del sistema.",
            },
      );
      void refresh();
    },
    onError: (cause) => setNotice({ kind: "error", message: getErrorMessage(cause) }),
  });
  const deleteKey = useMutation(
    run(api.deleteSteamApiKey, () => "La Web API Key se eliminó del almacén seguro."),
  );
  const unlink = useMutation(
    run(
      api.unlinkSteam,
      () => "La cuenta se ha desvinculado. Tus datos personales se conservan.",
      refreshSteamData,
    ),
  );
  return (
    <div className="settings-section">
      <SettingsHeading
        title="Cuenta de Steam"
        description="La autenticación se realiza en el navegador oficial de Steam. Vindexa nunca ve tu contraseña."
      />
      {notice ? (
        <InlineNotice {...notice} />
      ) : account?.lastSyncErrorMessage ? (
        <InlineNotice kind="error" message={account.lastSyncErrorMessage} />
      ) : null}
      {account ? (
        <div className="steam-account-card">
          <div className="steam-account-card__identity">
            {account.avatarUrl ? (
              <img src={account.avatarUrl} alt="" />
            ) : (
              <IconBrandSteam size={34} />
            )}
            <div>
              <strong>{account.personaName ?? "Cuenta de Steam"}</strong>
              {/* Un SteamID64 son diecisiete cifras: copiarlo a mano de una
                  pantalla es justo el tipo de trabajo que no debería existir. */}
              <span>
                SteamID64 ·{" "}
                <CopyableValue
                  value={account.steamId}
                  label={`Copiar el SteamID64 ${account.steamId}`}
                />
              </span>
              <span>Última sincronización · {formatDate(account.lastSyncAt)}</span>
            </div>
          </div>
          <div className="button-row">
            <Button
              size="sm"
              onClick={() => sync.mutate()}
              disabled={
                sync.isPending ||
                saveKey.isPending ||
                deleteKey.isPending ||
                unlink.isPending ||
                localImport.isPending
              }
            >
              {sync.isPending ? <IconLoader2 className="is-spinning" /> : <IconRefresh />}{" "}
              Sincronizar ahora
            </Button>
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button variant="outline" size="sm" disabled={sync.isPending || unlink.isPending}>
                  <IconTrash /> Desvincular
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>¿Desvincular la cuenta?</AlertDialogTitle>
                  <AlertDialogDescription>
                    Se eliminará la identidad vinculada y el acceso a futuras sincronizaciones. Tus
                    estados, notas, colecciones y progreso personal permanecerán intactos. El
                    catálogo derivado de Steam Family se retirará del equipo.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancelar</AlertDialogCancel>
                  <AlertDialogAction
                    disabled={sync.isPending || unlink.isPending}
                    onClick={() => {
                      if (!sync.isPending) unlink.mutate();
                    }}
                  >
                    Desvincular
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          </div>
        </div>
      ) : (
        <div className="settings-callout">
          <IconBrandSteam aria-hidden="true" size={30} />
          <div>
            <strong>Vincula tu biblioteca oficial</strong>
            <p>Abre Steam OpenID en tu navegador para identificar tu cuenta de forma segura.</p>
          </div>
          <Button size="sm" onClick={() => login.mutate()} disabled={login.isPending}>
            {login.isPending ? <IconLoader2 className="is-spinning" /> : <IconBrandSteam />}{" "}
            Continuar con Steam
          </Button>
        </div>
      )}
      <SettingsDivider />
      <SettingsHeading
        title="Historial de sincronización"
        description="Últimas ejecuciones locales y de Steam, con su resultado."
      />
      <SyncHistory />
      <SettingsDivider />
      <SettingsHeading
        title="Web API Key"
        description="Algunas bibliotecas requieren una clave personal para obtener juegos y tiempo jugado. Se almacena en Keychain, nunca en SQLite ni en la interfaz."
      />
      {bootstrap?.steam.apiKeyVerificationRequired && (
        <div className="settings-callout">
          <IconShieldLock aria-hidden="true" size={26} />
          <div>
            <strong>Comprueba si ya tienes una clave guardada</strong>
            <p>
              Por privacidad, Vindexa no consulta Keychain al iniciar. Esta acción voluntaria accede
              una vez al almacén seguro y solo guarda localmente si encontró una clave.
            </p>
          </div>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={() => verifyKey.mutate()}
            disabled={verifyKey.isPending || sync.isPending}
          >
            {verifyKey.isPending ? <IconLoader2 className="is-spinning" /> : <IconShieldLock />}{" "}
            Comprobar clave guardada
          </Button>
        </div>
      )}
      <form
        className="api-key-form"
        onSubmit={form.handleSubmit((values) => {
          if (!saveKey.isPending && !sync.isPending) saveKey.mutate(values);
        })}
      >
        <label className="sr-only" htmlFor="steam-api-key">
          Web API Key de Steam
        </label>
        <div className="secure-input">
          <IconKey aria-hidden="true" size={16} />
          <Input
            id="steam-api-key"
            {...form.register("apiKey")}
            type={showKey ? "text" : "password"}
            autoComplete="off"
            placeholder={
              bootstrap?.steam.apiKeyConfigured
                ? "Clave configurada · introduce otra para sustituirla"
                : "Introduce tu Web API Key"
            }
            aria-invalid={Boolean(form.formState.errors.apiKey)}
          />
          <button
            type="button"
            aria-label={showKey ? "Ocultar clave" : "Mostrar clave"}
            onClick={() => setShowKey((value) => !value)}
          >
            {showKey ? <IconEyeOff size={16} /> : <IconEye size={16} />}
          </button>
        </div>
        {form.formState.errors.apiKey && (
          <p className="field-error">{form.formState.errors.apiKey.message}</p>
        )}
        <div className="button-row">
          <Button size="sm" type="submit" disabled={saveKey.isPending || sync.isPending}>
            {saveKey.isPending ? <IconLoader2 className="is-spinning" /> : <IconShieldLock />}{" "}
            {account ? "Guardar y sincronizar" : "Guardar de forma segura"}
          </Button>
          {bootstrap?.steam.apiKeyConfigured ? (
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={deleteKey.isPending || sync.isPending}
                >
                  Eliminar clave
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>¿Eliminar la Web API Key?</AlertDialogTitle>
                  <AlertDialogDescription>
                    Tu cuenta vinculada, biblioteca y datos locales permanecerán intactos, pero las
                    sincronizaciones con Steam se detendrán hasta que guardes otra clave.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancelar</AlertDialogCancel>
                  <AlertDialogAction
                    disabled={deleteKey.isPending || sync.isPending}
                    onClick={() => {
                      if (!sync.isPending) deleteKey.mutate();
                    }}
                  >
                    Eliminar clave
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          ) : (
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => openUrl("https://steamcommunity.com/dev/apikey")}
            >
              <IconExternalLink /> Obtener una Web API Key
            </Button>
          )}
        </div>
        {!bootstrap?.steam.apiKeyConfigured && (
          <p className="settings-note">
            Steam abrirá su página oficial y puede pedirte que inicies sesión.
          </p>
        )}
      </form>
      <SettingsDivider />
      <SettingsHeading
        title="Bibliotecas instaladas"
        description={
          bootstrap?.steam.localSteamDetected
            ? `Steam detectado con ${bootstrap.steam.localManifestCount} manifiestos locales.`
            : "Todavía no se ha detectado una instalación local de Steam."
        }
      />
      <Button
        variant="secondary"
        size="sm"
        onClick={() => localImport.mutate()}
        disabled={localImport.isPending || sync.isPending}
      >
        {localImport.isPending ? <IconLoader2 className="is-spinning" /> : <IconFolderOpen />}{" "}
        Explorar bibliotecas locales
      </Button>
      <SettingsDivider />
      <FamilySessionSettings />
    </div>
  );
}

/**
 * Catálogo completo de Steam Family.
 *
 * La sincronización normal pregunta por cada miembro con la Web API Key, y eso
 * sólo devuelve algo de quien tenga la biblioteca pública. Casi nadie la tiene,
 * así que faltaban miles de juegos que el cliente de Steam sí enseña. Esta vía
 * usa el testigo de la sesión abierta en el navegador integrado, que es lo que
 * usa el propio cliente, y ve el catálogo entero.
 */
function FamilySessionSettings() {
  const queryClient = useQueryClient();
  const [notice, setNotice] = useState<{ kind: "success" | "error"; message: string }>();

  const status = useQuery({
    queryKey: ["steam-family-session"],
    queryFn: api.steamFamilySessionStatus,
  });

  const refrescar = async () => {
    await queryClient.invalidateQueries({ queryKey: ["steam-family-session"] });
  };

  const vincular = useMutation({
    mutationFn: api.linkSteamFamilySession,
    onSuccess: () =>
      setNotice({
        kind: "success",
        message: "Sesión de Steam vinculada. Ya puedes traer el catálogo de tu Familia.",
      }),
    onError: (error: unknown) => setNotice({ kind: "error", message: getErrorMessage(error) }),
    onSettled: () => void refrescar(),
  });

  const desvincular = useMutation({
    mutationFn: api.unlinkSteamFamilySession,
    onSuccess: () =>
      setNotice({
        kind: "success",
        message: "Sesión olvidada. El catálogo ya importado se conserva.",
      }),
    onError: (error: unknown) => setNotice({ kind: "error", message: getErrorMessage(error) }),
    onSettled: () => void refrescar(),
  });

  const sincronizar = useMutation({
    mutationFn: api.syncSteamFamilyCatalog,
    onSuccess: (report) => {
      setNotice({ kind: "success", message: describeFamilySync(report) });
      invalidateSteamDerivedQueries(queryClient);
    },
    onError: (error: unknown) => setNotice({ kind: "error", message: getErrorMessage(error) }),
    onSettled: () => void refrescar(),
  });

  const ocupado = vincular.isPending || desvincular.isPending || sincronizar.isPending;
  const vinculado = status.data?.linked ?? false;

  return (
    <>
      <SettingsHeading
        title="Catálogo de Steam Family"
        description={describeFamilyStatus(status.data)}
      />
      {notice && <InlineNotice kind={notice.kind} message={notice.message} />}
      <div className="button-row">
        <Button size="sm" disabled={ocupado} onClick={() => vincular.mutate()}>
          {vincular.isPending ? <IconLoader2 className="is-spinning" /> : <IconUsersGroup />}{" "}
          {vinculado ? "Renovar la sesión" : "Vincular la sesión de Steam"}
        </Button>
        {vinculado && (
          <>
            <Button
              size="sm"
              variant="secondary"
              disabled={ocupado}
              onClick={() => sincronizar.mutate()}
            >
              {sincronizar.isPending ? <IconLoader2 className="is-spinning" /> : <IconRefresh />}{" "}
              {sincronizar.isPending ? "Preguntando a Steam…" : "Traer el catálogo"}
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={ocupado}
              onClick={() => desvincular.mutate()}
            >
              <IconUnlink /> Olvidar la sesión
            </Button>
          </>
        )}
      </div>
      <p className="settings-note">
        Se abrirá el navegador integrado en Steam. Si no has iniciado sesión allí, hazlo y vuelve a
        pulsar. Vindexa sólo lee el testigo de sesión: ni cookies, ni contraseña, ni ningún dato de
        los demás miembros de la Familia.
      </p>
    </>
  );
}
function AppearanceSettings({ bootstrap }: { bootstrap?: AppBootstrap | undefined }) {
  const queryClient = useQueryClient();
  const [preferences, setPreferences] = useState<AppPreferences>(
    bootstrap?.preferences ?? {
      density: "compact",
      periodicSyncMinutes: 60,
      confirmUninstall: true,
      librarySort: "manual",
      artCacheMib: 0,
      shortcuts: DEFAULT_SHORTCUTS,
    },
  );
  useEffect(() => {
    if (bootstrap) setPreferences(bootstrap.preferences);
  }, [bootstrap]);
  const mutation = useMutation({
    mutationFn: (next: AppPreferences) => api.savePreferences(next),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["bootstrap"] }),
  });
  const update = (next: AppPreferences) => {
    setPreferences(next);
    mutation.mutate(next);
  };
  return (
    <div className="settings-section">
      <SettingsHeading
        title="Apariencia y comportamiento"
        description="Ajusta la densidad sin perder la arquitectura compacta de la biblioteca."
      />
      <SettingRow
        label="Densidad"
        description="Cambia la altura y separación de filas, controles y portadas."
      >
        <Select
          value={preferences.density}
          onValueChange={(density: AppPreferences["density"]) =>
            update({ ...preferences, density })
          }
        >
          <SelectTrigger className="w-40" aria-label="Densidad de la interfaz">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="compact">Compacta</SelectItem>
            <SelectItem value="comfortable">Cómoda</SelectItem>
          </SelectContent>
        </Select>
      </SettingRow>
      <SettingRow
        label="Confirmar desinstalación"
        description="Pide confirmación en Vindexa antes de entregar la solicitud al cliente de Steam."
      >
        <Switch
          aria-label="Confirmar desinstalación"
          checked={preferences.confirmUninstall}
          onCheckedChange={(confirmUninstall) => update({ ...preferences, confirmUninstall })}
        />
      </SettingRow>
      <SettingRow
        label="Caché de arte"
        description="Vindexa guarda las portadas y los banners oficiales en su máxima resolución para que la biblioteca se pinte desde el disco, sin volver a descargarlos. Sin límite es lo normal: sólo ocupa lo que tiene tu biblioteca, y nunca se come el espacio que tu sistema necesita para funcionar."
      >
        <Select
          value={String(preferences.artCacheMib)}
          onValueChange={(mib) => update({ ...preferences, artCacheMib: Number(mib) })}
        >
          <SelectTrigger className="w-40" aria-label="Techo de la caché de arte">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="0">Sin límite</SelectItem>
            <SelectItem value="128">128 MiB</SelectItem>
            <SelectItem value="256">256 MiB</SelectItem>
            <SelectItem value="512">512 MiB</SelectItem>
            <SelectItem value="1024">1 GiB</SelectItem>
            <SelectItem value="2048">2 GiB</SelectItem>
            <SelectItem value="4096">4 GiB</SelectItem>
            <SelectItem value="8192">8 GiB</SelectItem>
            <SelectItem value="16384">16 GiB</SelectItem>
            <SelectItem value="32768">32 GiB</SelectItem>
            <SelectItem value="65536">64 GiB</SelectItem>
          </SelectContent>
        </Select>
      </SettingRow>
      <SettingRow
        label="Sincronización periódica"
        description="Intervalo entre comprobaciones automáticas cuando Steam está vinculado."
      >
        <Select
          value={String(preferences.periodicSyncMinutes)}
          onValueChange={(minutes) =>
            update({ ...preferences, periodicSyncMinutes: Number(minutes) })
          }
        >
          <SelectTrigger className="w-40" aria-label="Intervalo de sincronización">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="0">Solo manual</SelectItem>
            <SelectItem value="30">Cada 30 min</SelectItem>
            <SelectItem value="60">Cada hora</SelectItem>
            <SelectItem value="360">Cada 6 horas</SelectItem>
            <SelectItem value="1440">Cada día</SelectItem>
          </SelectContent>
        </Select>
      </SettingRow>
      <p className="settings-save-state">
        {mutation.isPending
          ? "Guardando…"
          : mutation.isError
            ? getErrorMessage(mutation.error)
            : mutation.isSuccess
              ? "Guardado"
              : "Los cambios se guardan automáticamente."}
      </p>
    </div>
  );
}

const SHORTCUT_SCOPES: readonly ShortcutScope[] = ["navigation", "library", "game"];

function ShortcutSettings({ bootstrap }: { bootstrap?: AppBootstrap | undefined }) {
  const queryClient = useQueryClient();
  const [overrides, setOverrides] =
    useState<Partial<Record<LocalShortcutAction, string>>>(readLocalShortcuts);
  const [recording, setRecording] = useState<AnyShortcutAction>();
  const [notice, setNotice] = useState<{ kind: "success" | "error"; message: string }>();
  const bindings = useMemo(
    () => resolveShortcuts(bootstrap?.preferences.shortcuts, overrides),
    [bootstrap?.preferences.shortcuts, overrides],
  );
  const mutation = useMutation({
    mutationFn: (next: AppPreferences) => api.savePreferences(next),
    onSuccess: () => {
      setNotice({ kind: "success", message: "Atajos guardados." });
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    },
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });
  useEffect(() => {
    if (!recording) return;
    const capture = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      // Mientras se graba, este manejador se queda con todas las teclas: sin una
      // salida, cambiar de opinión obligaría a asignar algo que no se quiere o a
      // cerrar el diálogo, porque el propio Esc del diálogo tampoco llega.
      // `Esc` a secas cancela; con modificador sigue siendo asignable.
      if (
        event.key === "Escape" &&
        !event.metaKey &&
        !event.ctrlKey &&
        !event.altKey &&
        !event.shiftKey
      ) {
        setRecording(undefined);
        setNotice(undefined);
        return;
      }
      const shortcut = eventToShortcut(event);
      if (!shortcut) return;
      const rejection = describeShortcutRejection(bindings, recording, shortcut);
      if (rejection) {
        setNotice({ kind: "error", message: rejection });
        setRecording(undefined);
        return;
      }
      const descriptor = SHORTCUT_CATALOGUE.find((entry) => entry.action === recording);
      setRecording(undefined);
      setNotice(undefined);
      if (descriptor?.persistence === "local") {
        const next = { ...overrides, [recording as LocalShortcutAction]: shortcut };
        setOverrides(next);
        writeLocalShortcuts(next);
        setNotice({ kind: "success", message: "Atajos guardados." });
        return;
      }
      if (!bootstrap) return;
      const persisted: ShortcutBindings = {
        ...bootstrap.preferences.shortcuts,
        [recording]: shortcut,
      };
      // `Mod+K` era el valor de fábrica de la búsqueda antes de existir la
      // paleta. Si la nueva asignación choca con ese resto, se migra en el
      // mismo guardado: Rust rechaza cualquier mapa con duplicados.
      if (
        persisted.search === LEGACY_SEARCH_SHORTCUT &&
        findShortcutCollision(persisted, "search", persisted.search)
      ) {
        persisted.search = DEFAULT_SHORTCUTS.search;
      }
      mutation.mutate({ ...bootstrap.preferences, shortcuts: persisted });
    };
    window.addEventListener("keydown", capture, true);
    return () => window.removeEventListener("keydown", capture, true);
  }, [bindings, bootstrap, mutation, overrides, recording]);
  const reset = () => {
    setRecording(undefined);
    setNotice(undefined);
    setOverrides({});
    writeLocalShortcuts({});
    if (bootstrap) {
      mutation.mutate({ ...bootstrap.preferences, shortcuts: { ...DEFAULT_SHORTCUTS } });
    } else {
      setNotice({ kind: "success", message: "Atajos guardados." });
    }
  };
  return (
    <div className="settings-section">
      <SettingsHeading
        title="Atajos de teclado"
        description="Selecciona una acción y pulsa la nueva combinación. Las colisiones se rechazan antes de guardar."
      />
      {notice && <InlineNotice {...notice} />}
      {SHORTCUT_SCOPES.map((scope, index) => (
        <Fragment key={scope}>
          {index > 0 && <SettingsDivider />}
          <SettingsHeading
            title={SHORTCUT_SCOPE_LABELS[scope]}
            description={SHORTCUT_SCOPE_HINTS[scope]}
          />
          <div className="shortcut-list">
            {SHORTCUT_CATALOGUE.filter((entry) => entry.scope === scope).map((entry) => (
              <div className="shortcut-row" key={entry.action}>
                <span>{entry.label}</span>
                <Button
                  type="button"
                  size="sm"
                  variant={recording === entry.action ? "secondary" : "outline"}
                  aria-label={`Cambiar ${entry.label}`}
                  aria-pressed={recording === entry.action}
                  onClick={() => {
                    setNotice(undefined);
                    setRecording((actual) => (actual === entry.action ? undefined : entry.action));
                  }}
                >
                  {recording === entry.action ? (
                    "Pulsa una combinación…"
                  ) : bindings[entry.action] ? (
                    <kbd>{shortcutLabel(bindings[entry.action])}</kbd>
                  ) : (
                    "Sin asignar"
                  )}
                </Button>
              </div>
            ))}
          </div>
        </Fragment>
      ))}
      <div className="button-row">
        <Button
          type="button"
          size="sm"
          variant="ghost"
          onClick={reset}
          disabled={mutation.isPending}
        >
          Restablecer atajos
        </Button>
      </div>
      <p className="settings-note">
        Los atajos no se ejecutan mientras escribes en campos, áreas de texto o editores. Los de
        biblioteca y juego sólo actúan sobre la rejilla o la lista, nunca sobre el control que tenga
        el foco en otra parte de la aplicación.
      </p>
    </div>
  );
}

function DataSettings({ bootstrap }: { bootstrap?: AppBootstrap | undefined }) {
  const queryClient = useQueryClient();
  const [notice, setNotice] = useState<{ kind: "success" | "error"; message: string }>();
  const diagnostics = useQuery({
    queryKey: ["diagnostics"],
    queryFn: api.diagnostics,
    enabled: true,
  });
  const backup = useMutation({
    mutationFn: api.exportBackup,
    onSuccess: (completed) => {
      if (completed) {
        setNotice({ kind: "success", message: "Copia creada y verificada correctamente." });
      }
    },
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });
  const restore = useMutation({
    mutationFn: api.importBackup,
    onSuccess: (completed) => {
      if (completed) {
        setNotice({ kind: "success", message: "Copia restaurada y verificada correctamente." });
        void queryClient.invalidateQueries();
      }
    },
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });
  const maintainCache = useMutation({
    mutationFn: api.maintainArtCache,
    onSuccess: (report) =>
      setNotice({
        kind: "success",
        message: `Caché depurada: ${report.removedFiles + report.evictedFiles} archivo${
          report.removedFiles + report.evictedFiles === 1 ? "" : "s"
        } liberado${report.removedFiles + report.evictedFiles === 1 ? "" : "s"}; ocupa ${formatBytes(
          report.bytesAfter,
        )}.`,
      }),
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });
  const clearCache = useMutation({
    mutationFn: api.clearArtCache,
    onSuccess: () =>
      setNotice({
        kind: "success",
        message: "La caché de imágenes se ha vaciado; se reconstruirá bajo demanda.",
      }),
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });
  return (
    <div className="settings-section">
      <SettingsHeading
        title="Datos y copias de seguridad"
        description="SQLite es la fuente de verdad. Las restauraciones crean antes una copia de seguridad automática."
      />
      {notice && <InlineNotice {...notice} />}
      <div className="button-row">
        <Button size="sm" onClick={() => backup.mutate()} disabled={backup.isPending}>
          <IconDownload /> Exportar copia
        </Button>
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button size="sm" variant="outline" disabled={restore.isPending}>
              <IconUpload /> Restaurar copia
            </Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>¿Seleccionar una copia para restaurarla?</AlertDialogTitle>
              <AlertDialogDescription>
                La restauración sustituirá la base activa. Antes de aplicar ningún cambio, Vindexa
                creará y verificará automáticamente un snapshot de seguridad de la base actual.
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>Cancelar</AlertDialogCancel>
              <AlertDialogAction onClick={() => restore.mutate()}>
                Elegir copia y restaurar
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => clearCache.mutate()}
          disabled={clearCache.isPending}
        >
          <IconRefresh /> Vaciar caché de imágenes
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => maintainCache.mutate()}
          disabled={maintainCache.isPending}
        >
          <IconRefresh /> Depurar caché
        </Button>
      </div>
      <p className="settings-hint">
        Vindexa guarda el arte oficial en su máxima resolución. Depurar elimina lo huérfano y
        respeta el presupuesto en disco configurado en Apariencia; vaciar obliga a descargarlo todo
        de nuevo bajo demanda.
      </p>
      <SettingsDivider />
      <SettingsHeading
        title="Diagnóstico local"
        description="Estado técnico de la base de datos activa."
      />
      <dl className="diagnostics-grid">
        <div>
          <dt>Integridad</dt>
          <dd>{diagnostics.data?.integrity ?? "Comprobando…"}</dd>
        </div>
        <div>
          <dt>Modo WAL</dt>
          <dd>{diagnostics.data?.walEnabled ? "Activo" : "No disponible"}</dd>
        </div>
        <div>
          <dt>Esquema</dt>
          <dd>v{diagnostics.data?.schemaVersion ?? "—"}</dd>
        </div>
        <div>
          <dt>Tamaño</dt>
          <dd>{formatBytes(diagnostics.data?.sizeBytes)}</dd>
        </div>
      </dl>
      <label className="path-field" htmlFor="database-path">
        <span>Ubicación de datos</span>
        <Input
          id="database-path"
          value={diagnostics.data?.path ?? bootstrap?.databasePath ?? ""}
          readOnly
        />
      </label>
    </div>
  );
}

function PrivacySettings() {
  return (
    <div className="settings-section">
      <SettingsHeading
        title="Privacidad por diseño"
        description="Tu organización personal permanece en este equipo."
      />
      <ul className="privacy-list">
        <li>
          <IconCheck /> La contraseña de Steam nunca entra en Vindexa.
        </li>
        <li>
          <IconCheck /> La Web API Key vive en el almacén seguro del sistema.
        </li>
        <li>
          <IconCheck /> Notas, checkpoints y colecciones se guardan únicamente en SQLite local.
        </li>
        <li>
          <IconCheck /> Las sincronizaciones no sobrescriben tu organización personal.
        </li>
      </ul>

      {/* Una página de privacidad que sólo enumera lo que se queda aquí dice
          media verdad: Vindexa **sí** habla con las tiendas, y para algunas
          cosas les dice de qué juegos habla. Lo que sale se dice con la misma
          claridad que lo que se queda. */}
      <SettingsHeading
        title="Qué sale de este equipo"
        description="Vindexa pregunta a las tiendas por datos públicos. Esto es todo lo que viaja."
      />
      {/* El texto va dentro de un solo `span`: con las frases sueltas, el `flex`
          del contenedor convertía cada trozo en una columna y el «los AppID de
          tus juegos» se apilaba en vertical al lado del resto. */}
      <ul className="privacy-list" data-tone="outbound">
        <li>
          <IconWorld />
          <span>
            <b>Los AppID de tus juegos y deseados</b>, a la tienda pública de Steam, para traer
            precios, fichas, capturas, publicaciones oficiales y la marca DRM. Van los
            identificadores, no tu cuenta.
          </span>
        </li>
        <li>
          <IconWorld />
          <span>
            <b>Nada tuyo</b> a Epic ni a GOG: sus catálogos de regalos y rebajas se piden igual para
            todo el mundo, sin identificarte.
          </span>
        </li>
        <li>
          <IconWorld />
          <span>
            <b>Tu SteamID y tu Web API Key</b>, sólo a Steam y sólo si vinculas la cuenta, para
            traer tiempo jugado y logros.
          </span>
        </li>
        <li>
          <IconWorld />
          <span>
            <b>Lo que navegues</b> en el navegador integrado, a esa tienda y con la sesión que tú
            hayas iniciado en ella. Cada tienda tiene su almacén aislado.
          </span>
        </li>
      </ul>
      <p className="settings-note">
        El modelo de gustos, las notas, los estados y las colecciones no salen: se calculan y se
        guardan aquí. Vindexa no tiene servidor propio, ni telemetría, ni cuenta de Vindexa.
      </p>
    </div>
  );
}
function AboutSettings({ bootstrap }: { bootstrap?: AppBootstrap | undefined }) {
  const [notice, setNotice] = useState<{ kind: "success" | "error"; message: string }>();
  const [versionNueva, setVersionNueva] = useState<{ page: string }>();
  const updateCheck = useMutation({
    mutationFn: api.checkForUpdates,
    onSuccess: (result) => {
      setVersionNueva(result.status === "available" ? { page: result.releasePage } : undefined);
      // No poder comprobarlo se enseña como error, porque no saber si estás al
      // día no es estar al día.
      setNotice({
        kind: result.status === "unreachable" ? "error" : "success",
        message: result.message,
      });
    },
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });
  return (
    <div className="settings-section">
      <SettingsHeading
        title="Vindexa"
        description="Un índice personal para decidir mejor qué jugar, continuar y terminar."
      />
      <dl className="about-grid">
        <div>
          <dt>Versión</dt>
          <dd>{bootstrap?.appVersion ?? "—"}</dd>
        </div>
        <div>
          <dt>Motor</dt>
          <dd>Tauri 2 · React 19</dd>
        </div>
        <div>
          <dt>Persistencia</dt>
          <dd>SQLite local</dd>
        </div>
        <div>
          <dt>Interfaz</dt>
          <dd>Español de España</dd>
        </div>
      </dl>
      {notice && <InlineNotice {...notice} />}
      <Button
        size="sm"
        variant="secondary"
        onClick={() => updateCheck.mutate()}
        disabled={updateCheck.isPending}
      >
        {updateCheck.isPending ? <IconLoader2 className="is-spinning" /> : <IconRefresh />}
        Buscar actualizaciones
      </Button>
      {versionNueva && (
        <Button size="sm" variant="secondary" onClick={() => openUrl(versionNueva.page)}>
          <IconExternalLink /> Ver la versión publicada
        </Button>
      )}
      <p className="settings-note">
        La comprobación es manual y sólo mira qué versión hay publicada. Vindexa no descarga ni
        instala nada por su cuenta: hacerlo exigiría firmar los instaladores y llevar la clave
        pública dentro de la aplicación, y eso todavía no existe.
      </p>
      <p className="settings-note">
        Vindexa no está afiliada a Valve Corporation. Steam y sus marcas pertenecen a sus
        respectivos titulares.
      </p>
    </div>
  );
}

function SettingsHeading({ title, description }: { title: string; description: string }) {
  return (
    <div className="settings-heading">
      <h3>{title}</h3>
      <p>{description}</p>
    </div>
  );
}
function SettingsDivider() {
  return <div className="settings-divider" />;
}
function SettingRow({
  label,
  description,
  children,
}: {
  label: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <div className="setting-row">
      <div>
        <strong>{label}</strong>
        <span>{description}</span>
      </div>
      {children}
    </div>
  );
}
function InlineNotice({ kind, message }: { kind: "success" | "error"; message: string }) {
  return (
    <div className="inline-notice" data-kind={kind} role={kind === "error" ? "alert" : "status"}>
      {kind === "success" ? <IconCheck /> : <IconAlertCircle />}
      <span>{message}</span>
    </div>
  );
}
function SyncHistory() {
  const runs = useQuery({ queryKey: ["sync-runs"], queryFn: () => api.listSyncRuns(8) });
  if (!runs.data?.length) {
    return (
      <p className="sync-history-empty">
        Aún no hay ejecuciones registradas; aparecerán aquí tras la próxima sincronización o
        importación local.
      </p>
    );
  }
  return (
    <ul className="sync-history">
      {runs.data.map((run) => (
        <li key={run.id} data-status={run.status}>
          <span className="sync-history__dot" aria-hidden="true" />
          <div>
            <strong>
              {run.source === "local" ? "Importación local" : "Sincronización con Steam"}
            </strong>
            <span>{formatRelativeDate(run.startedAt)}</span>
          </div>
          {run.status === "success" ? (
            <data>
              +{run.importedCount} · {run.updatedCount} actualizados
            </data>
          ) : (
            <data title={run.errorMessage ?? undefined}>Error — {run.errorMessage}</data>
          )}
        </li>
      ))}
    </ul>
  );
}
