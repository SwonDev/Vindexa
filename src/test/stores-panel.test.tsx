import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { StoresPanel } from "@/features/settings/StoresPanel";
import { api } from "@/lib/tauri";
import type {
  ExternalGame,
  ExternalStoreAccount,
  ExternalStoreScanReport,
  StoreDetection,
} from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    detectExternalStores: vi.fn(),
    listExternalStoreAccounts: vi.fn(),
    listExternalGames: vi.fn(),
    scanExternalStore: vi.fn(),
    scanExternalStores: vi.fn(),
    setExternalGameMatch: vi.fn(),
    clearExternalGameMatch: vi.fn(),
    launchExternalGame: vi.fn(),
    linkExternalStore: vi.fn(),
    unlinkExternalStore: vi.fn(),
    rematchExternalStores: vi.fn(),
    listGames: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

const mockedApi = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

/**
 * Las órdenes de sesión de cuenta viajan por `invoke` directamente, igual que
 * las de itch.io, porque su contrato todavía no vive en `src/lib/tauri.ts`.
 */
const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

/** Sesión de cuenta tal y como la devuelve Rust: sin un solo campo secreto. */
interface StoreSessionFixture {
  store: string;
  displayName: string;
  signedIn: boolean;
  accountName: string | null;
  expiresAt: string | null;
  needsRefresh: boolean;
  refreshExpired: boolean;
  keychainService: string;
  keychainAccount: string;
  accountSessionsUrl: string | null;
  supportsInAppLogin: boolean;
}

function session(overrides: Partial<StoreSessionFixture> = {}): StoreSessionFixture {
  return {
    store: "epic",
    displayName: "Epic Games Store",
    signedIn: false,
    accountName: null,
    expiresAt: null,
    needsRefresh: false,
    refreshExpired: false,
    keychainService: "io.vindexa.desktop",
    keychainAccount: "epic-store-session",
    accountSessionsUrl: "https://www.epicgames.com/account/personal",
    supportsInAppLogin: true,
    ...overrides,
  };
}

const SIGNED_OUT_SESSIONS: StoreSessionFixture[] = [
  session(),
  session({
    store: "gog",
    displayName: "GOG",
    keychainAccount: "gog-store-session",
    accountSessionsUrl: "https://www.gog.com/account/settings/security",
  }),
];

/** Estado inerte de itch.io: su panel comparte pantalla pero no esta prueba. */
const ITCH_IDLE = {
  hasKey: false,
  account: null,
  lastImportAt: null,
  lastImportStatus: null,
  lastImportErrorMessage: null,
  gameCount: 0,
};

/**
 * Encamina cada orden a su respuesta. Se declara como función para que cada
 * prueba pueda sustituir sólo las sesiones sin reescribir el resto.
 */
function routeInvoke(sessions: StoreSessionFixture[] = SIGNED_OUT_SESSIONS) {
  invoke.mockImplementation((command: string) => {
    if (command === "list_external_store_sessions") return Promise.resolve(sessions);
    if (command === "itch_session_state") return Promise.resolve(ITCH_IDLE);
    return Promise.reject(new Error(`Orden no simulada: ${command}`));
  });
}

const NOTHING_DETECTED: StoreDetection[] = [
  {
    store: "epic",
    displayName: "Epic Games Store",
    detected: false,
    detectedPaths: [],
    searchedPaths: [
      "/Users/prueba/Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests",
      "/Users/prueba/.config/legendary/installed.json",
    ],
  },
  {
    store: "gog",
    displayName: "GOG",
    detected: false,
    detectedPaths: [],
    searchedPaths: ["/Users/Shared/GOG.com/Galaxy/storage/galaxy-2.0.db"],
  },
];

const EPIC_DETECTED: StoreDetection[] = [
  {
    store: "epic",
    displayName: "Epic Games Store",
    detected: true,
    detectedPaths: ["/Users/prueba/Library/Application Support/Epic/.../Manifests"],
    searchedPaths: ["/Users/prueba/Library/Application Support/Epic/.../Manifests"],
  },
  NOTHING_DETECTED[1] as StoreDetection,
];

function account(overrides: Partial<ExternalStoreAccount> = {}): ExternalStoreAccount {
  return {
    store: "epic",
    displayName: "Epic Games Store",
    detectedRoot: "/Users/prueba/Library/Application Support/Epic/.../Manifests",
    linked: false,
    lastScanAt: "2026-08-17T10:00:00Z",
    lastScanStatus: "success",
    lastScanErrorCode: null,
    lastScanErrorMessage: null,
    gameCount: 2,
    ...overrides,
  };
}

function externalGame(overrides: Partial<ExternalGame> = {}): ExternalGame {
  return {
    store: "epic",
    externalId: "Fortnite",
    title: "Hollow Knight",
    coverUrl: null,
    headerUrl: null,
    installPath: "/Juegos/Hollow Knight",
    installed: true,
    sizeOnDisk: 1024,
    launchTarget: null,
    drmState: "unknown",
    drmEvidence: [],
    matchedAppId: null,
    matchedTitle: null,
    matchConfidence: 0,
    matchSource: "none",
    discoveredAt: "2026-08-16T10:00:00Z",
    updatedAt: "2026-08-17T10:00:00Z",
    ...overrides,
  };
}

const SUCCESSFUL_SCAN: ExternalStoreScanReport = {
  store: "epic",
  status: "success",
  detectedRoot: "/Users/prueba/Library/Application Support/Epic/.../Manifests",
  discovered: 2,
  matched: 1,
  skipped: 0,
  expired: 0,
  sources: ["epicManifests"],
  errorCode: null,
  errorMessage: null,
};

function renderPanel() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <StoresPanel />
    </QueryClientProvider>,
  );
}

describe("panel de tiendas externas", () => {
  beforeEach(() => {
    mockedApi.detectExternalStores.mockResolvedValue(NOTHING_DETECTED);
    mockedApi.listExternalStoreAccounts.mockResolvedValue([]);
    mockedApi.listExternalGames.mockResolvedValue({
      items: [],
      total: 0,
      limit: 60,
      offset: 0,
    });
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 8, offset: 0 });
    mockedApi.rematchExternalStores.mockResolvedValue(0);
    invoke.mockReset();
    routeInvoke();
  });

  it("reintentar el emparejado no toca lo que la persona corrigió a mano", async () => {
    const user = userEvent.setup();
    mockedApi.rematchExternalStores.mockResolvedValue(3);
    renderPanel();

    await user.click(
      await screen.findByRole("button", { name: /Reintentar emparejado con Steam/ }),
    );
    expect(mockedApi.rematchExternalStores).toHaveBeenCalledTimes(1);
    expect(
      await screen.findByText(
        "3 emparejados automáticos actualizados. Tus correcciones manuales siguen intactas.",
      ),
    ).toBeVisible();
  });

  it("dice que no hay ninguna tienda y enseña las rutas exactas donde buscó", async () => {
    const user = userEvent.setup();
    renderPanel();

    const epicCard = await screen.findByRole("article", { name: "Epic Games Store" });
    // Que no haya cliente ya no es un obstáculo: la biblioteca llega por la
    // cuenta, así que la tarjeta lo dice sin dramatizarlo.
    expect(screen.getAllByText("No hace falta")).toHaveLength(2);
    expect(screen.getByText(/No hay ningún cliente de Epic ni de GOG/)).toBeVisible();

    // La ruta no se enseña de entrada, pero está a un clic y es la real.
    await user.click(within(epicCard).getByRole("button", { name: /Ver dónde se busca/ }));
    expect(
      within(epicCard).getByText("/Users/prueba/.config/legendary/installed.json"),
    ).toBeVisible();
  });

  it("no inventa juegos cuando todavía no se ha escaneado nada", async () => {
    renderPanel();
    expect(
      await screen.findByText(
        "Todavía no hay ningún juego detectado. Escanea una tienda para leer sus manifiestos.",
      ),
    ).toBeVisible();
  });

  it("muestra recuento y último escaneo de la tienda detectada, y resume el escaneo", async () => {
    const user = userEvent.setup();
    mockedApi.detectExternalStores.mockResolvedValue(EPIC_DETECTED);
    mockedApi.listExternalStoreAccounts.mockResolvedValue([account()]);
    mockedApi.scanExternalStore.mockResolvedValue(SUCCESSFUL_SCAN);

    renderPanel();
    const epicCard = await screen.findByRole("article", { name: "Epic Games Store" });
    expect(within(epicCard).getByText("Detectado")).toBeVisible();
    expect(within(epicCard).getByText("2 juegos")).toBeVisible();
    expect(within(epicCard).getByText("Leído correctamente")).toBeVisible();

    await user.click(within(epicCard).getByRole("button", { name: /Escanear$/ }));
    expect(mockedApi.scanExternalStore).toHaveBeenCalledWith("epic");
    expect(
      await screen.findByText("Epic Games Store: 2 juegos leídos, 1 emparejados con Steam."),
    ).toBeVisible();
  });

  it("declara el motivo cuando el cliente está pero su biblioteca no se puede leer", async () => {
    mockedApi.detectExternalStores.mockResolvedValue(EPIC_DETECTED);
    mockedApi.listExternalStoreAccounts.mockResolvedValue([
      account({
        lastScanStatus: "failed",
        lastScanErrorCode: "epic_manifests_unreadable",
        lastScanErrorMessage: "Se encontró Epic, pero sus manifiestos no se pudieron leer.",
      }),
    ]);

    renderPanel();
    const epicCard = await screen.findByRole("article", { name: "Epic Games Store" });
    expect(within(epicCard).getByText("No se pudo leer")).toBeVisible();
    expect(
      within(epicCard).getByText("Se encontró Epic, pero sus manifiestos no se pudieron leer."),
    ).toBeVisible();
  });

  it("una tienda detectada sin sesión no dice a la vez «detectada» y «cliente no encontrado»", async () => {
    mockedApi.detectExternalStores.mockResolvedValue(EPIC_DETECTED);
    mockedApi.listExternalStoreAccounts.mockResolvedValue([
      account({
        gameCount: 0,
        lastScanStatus: "unavailable",
        lastScanErrorCode: "epic_not_signed_in",
        lastScanErrorMessage:
          "Se encontró Legendary o Heroic en este equipo, pero no hay ninguna biblioteca de Epic guardada.",
      }),
    ]);

    renderPanel();
    const epicCard = await screen.findByRole("article", { name: "Epic Games Store" });

    expect(within(epicCard).getByText("Detectado")).toBeVisible();
    // La contradicción exacta que veía la persona usuaria.
    expect(within(epicCard).queryByText("Cliente no encontrado")).toBeNull();
    // Lo dicen la cabecera y el resultado de la lectura, y ahora coinciden en
    // vez de contradecirse: ése era justo el defecto.
    expect(within(epicCard).getAllByText("Sin sesión iniciada")).toHaveLength(2);
    // Y el consejo ya no manda a instalar otro programa: manda a iniciar sesión
    // aquí, que es lo que de verdad resuelve el problema.
    expect(
      within(epicCard).getByText(/Inicia sesión en Epic Games Store para traer tu biblioteca/),
    ).toBeVisible();
    // El consejo viejo mandaba a otro programa. El diagnóstico del escáner de
    // disco sí puede seguir nombrándolo —es lo que encontró—, pero ya no es lo
    // que se propone hacer.
    expect(within(epicCard).queryByText(/Abre Heroic Games Launcher/)).toBeNull();
  });

  it("una tienda leída de verdad y sin juegos lo dice, en vez de fingir que falta el cliente", async () => {
    mockedApi.detectExternalStores.mockResolvedValue(EPIC_DETECTED);
    mockedApi.listExternalStoreAccounts.mockResolvedValue([
      account({ gameCount: 0, lastScanStatus: "success" }),
    ]);

    renderPanel();
    const epicCard = await screen.findByRole("article", { name: "Epic Games Store" });
    expect(within(epicCard).getByText("Leído: sin juegos")).toBeVisible();
    expect(within(epicCard).queryByText("Cliente no encontrado")).toBeNull();
  });

  it("una tienda ausente sí dice «cliente no encontrado» y qué instalar", async () => {
    mockedApi.listExternalStoreAccounts.mockResolvedValue([
      account({
        store: "gog",
        displayName: "GOG",
        gameCount: 0,
        detectedRoot: null,
        lastScanStatus: "unavailable",
        lastScanErrorCode: "gog_install_folder_empty",
        lastScanErrorMessage:
          "Sólo se encontró la carpeta donde Heroic o los instaladores de GOG dejan los juegos, y está vacía.",
      }),
    ]);

    renderPanel();
    const gogCard = await screen.findByRole("article", { name: "GOG" });
    expect(within(gogCard).getByText("No hace falta")).toBeVisible();
    // El resultado de la lectura de disco se sigue diciendo tal cual…
    expect(within(gogCard).getByText("Cliente no encontrado")).toBeVisible();
    // …pero lo que se propone hacer ya no es instalar nada.
    expect(
      within(gogCard).getByText(/Inicia sesión en GOG para traer tu biblioteca/),
    ).toBeVisible();
  });

  it("explica qué se guarda, dónde y cómo se revoca, sin mandar a instalar otro programa", async () => {
    renderPanel();
    expect(await screen.findByText("Qué guarda Vindexa, dónde, y cómo se revoca")).toBeVisible();
    expect(screen.getByText(/Vindexa nunca ve tu contraseña/)).toBeVisible();
    expect(screen.getByText(/y en ningún otro sitio/)).toBeVisible();

    // El discurso viejo ya no puede volver por descuido.
    expect(screen.queryByText(/Heroic Games Launcher/)).toBeNull();
    expect(screen.queryByText(/no implementa su propio inicio de sesión/)).toBeNull();
  });

  it("ofrece iniciar sesión en cada tienda y dice dónde acabará el testigo", async () => {
    renderPanel();
    const epicCard = await screen.findByRole("article", { name: "Epic Games Store" });

    expect(
      within(epicCard).getByRole("button", { name: /Iniciar sesión en Epic Games Store/ }),
    ).toBeVisible();
    // La custodia se declara antes de que exista ningún testigo, que es cuando
    // la persona decide si quiere que exista.
    expect(within(epicCard).getByText(/se guardará en el llavero de macOS/)).toBeVisible();
    expect(within(epicCard).getByText("epic-store-session")).toBeVisible();
    expect(within(epicCard).getByText("io.vindexa.desktop")).toBeVisible();
  });

  it("iniciar sesión en Epic no pide copiar nada: se resuelve dentro de Vindexa", async () => {
    const user = userEvent.setup();
    let resolveSignIn: ((value: StoreSessionFixture) => void) | undefined;
    invoke.mockImplementation((command: string) => {
      if (command === "list_external_store_sessions") return Promise.resolve(SIGNED_OUT_SESSIONS);
      if (command === "itch_session_state") return Promise.resolve(ITCH_IDLE);
      if (command === "sign_in_external_store") {
        return new Promise<StoreSessionFixture>((resolve) => {
          resolveSignIn = resolve;
        });
      }
      return Promise.reject(new Error(`Orden no simulada: ${command}`));
    });

    renderPanel();
    const epicCard = await screen.findByRole("article", { name: "Epic Games Store" });
    await user.click(
      within(epicCard).getByRole("button", { name: /Iniciar sesión en Epic Games Store/ }),
    );

    // Una sola orden, y ningún campo donde pegar un código: eso era justo lo
    // que ninguna persona normal podía completar.
    expect(invoke).toHaveBeenCalledWith("sign_in_external_store", { store: "epic" });
    expect(
      await within(epicCard).findByText(/Identifícate ahí y esto terminará solo/),
    ).toBeVisible();
    expect(within(epicCard).queryByLabelText(/Código de autorización/)).toBeNull();

    resolveSignIn?.(session({ signedIn: true, accountName: "Fulanita" }));
  });

  it("una tienda que aún no puede identificarse dentro de Vindexa lo dice y usa la vía manual", async () => {
    const user = userEvent.setup();
    invoke.mockImplementation((command: string) => {
      if (command === "list_external_store_sessions") {
        return Promise.resolve([session({ supportsInAppLogin: false }), SIGNED_OUT_SESSIONS[1]]);
      }
      if (command === "itch_session_state") return Promise.resolve(ITCH_IDLE);
      if (command === "begin_external_store_login") {
        return Promise.resolve({
          store: "epic",
          displayName: "Epic Games Store",
          url: "https://www.epicgames.com/id/login",
          instructions: "Inicia sesión con normalidad: Vindexa no ve tu contraseña.",
          fieldLabel: "Código de autorización de Epic",
        });
      }
      return Promise.reject(new Error(`Orden no simulada: ${command}`));
    });

    renderPanel();
    const epicCard = await screen.findByRole("article", { name: "Epic Games Store" });
    expect(within(epicCard).getByText(/se identifica en tu navegador/)).toBeVisible();

    // El mismo botón, pero no promete lo que no puede hacer: va a la vía manual.
    await user.click(
      within(epicCard).getByRole("button", { name: /Iniciar sesión en Epic Games Store/ }),
    );
    expect(invoke).toHaveBeenCalledWith("begin_external_store_login", { store: "epic" });
    expect(invoke).not.toHaveBeenCalledWith("sign_in_external_store", { store: "epic" });
  });

  it("la vía manual sigue disponible para quien prefiera su propio navegador", async () => {
    const user = userEvent.setup();
    invoke.mockImplementation((command: string) => {
      if (command === "list_external_store_sessions") return Promise.resolve(SIGNED_OUT_SESSIONS);
      if (command === "itch_session_state") return Promise.resolve(ITCH_IDLE);
      if (command === "begin_external_store_login") {
        return Promise.resolve({
          store: "epic",
          displayName: "Epic Games Store",
          url: "https://www.epicgames.com/id/login",
          instructions: "Inicia sesión con normalidad: Vindexa no ve tu contraseña.",
          fieldLabel: "Código de autorización de Epic",
        });
      }
      if (command === "complete_external_store_login") {
        return Promise.resolve(session({ signedIn: true, accountName: "Fulanita" }));
      }
      return Promise.reject(new Error(`Orden no simulada: ${command}`));
    });

    renderPanel();
    const epicCard = await screen.findByRole("article", { name: "Epic Games Store" });
    await user.click(within(epicCard).getByRole("button", { name: "Hazlo a mano" }));

    // Abrir la página es cosa de Rust: la interfaz sólo pide que se abra.
    expect(invoke).toHaveBeenCalledWith("begin_external_store_login", { store: "epic" });
    const field = await within(epicCard).findByLabelText("Código de autorización de Epic");
    await user.type(field, "0123456789abcdef0123456789abcdef");
    await user.click(within(epicCard).getByRole("button", { name: /Conectar/ }));

    expect(invoke).toHaveBeenCalledWith("complete_external_store_login", {
      store: "epic",
      code: "0123456789abcdef0123456789abcdef",
    });
    // El código es de un solo uso y no se queda a la vista.
    expect(within(epicCard).queryByLabelText("Código de autorización de Epic")).toBeNull();
  });

  it("con sesión iniciada ofrece sincronizar y cerrar sesión, y nombra la cuenta", async () => {
    const user = userEvent.setup();
    routeInvoke([
      session({
        signedIn: true,
        accountName: "Fulanita",
        expiresAt: "2026-08-19T10:00:00Z",
      }),
      SIGNED_OUT_SESSIONS[1] as StoreSessionFixture,
    ]);
    invoke.mockImplementation((command: string) => {
      if (command === "list_external_store_sessions") {
        return Promise.resolve([
          session({ signedIn: true, accountName: "Fulanita", expiresAt: "2026-08-19T10:00:00Z" }),
          SIGNED_OUT_SESSIONS[1],
        ]);
      }
      if (command === "itch_session_state") return Promise.resolve(ITCH_IDLE);
      if (command === "sync_external_store_library") return Promise.resolve(SUCCESSFUL_SCAN);
      return Promise.reject(new Error(`Orden no simulada: ${command}`));
    });

    renderPanel();
    const epicCard = await screen.findByRole("article", { name: "Epic Games Store" });
    expect(within(epicCard).getByText("Sesión iniciada como Fulanita")).toBeVisible();

    await user.click(within(epicCard).getByRole("button", { name: /Sincronizar biblioteca/ }));
    expect(invoke).toHaveBeenCalledWith("sync_external_store_library", { store: "epic" });
    expect(within(epicCard).getByRole("button", { name: /Cerrar sesión/ })).toBeVisible();
  });

  it("cerrar sesión cuenta lo que se ha borrado y lo que no ha podido revocar", async () => {
    const user = userEvent.setup();
    invoke.mockImplementation((command: string) => {
      if (command === "list_external_store_sessions") {
        return Promise.resolve([
          session({ signedIn: true, accountName: "Fulanita" }),
          SIGNED_OUT_SESSIONS[1],
        ]);
      }
      if (command === "itch_session_state") return Promise.resolve(ITCH_IDLE);
      if (command === "sign_out_external_store") {
        return Promise.resolve({
          store: "epic",
          displayName: "Epic Games Store",
          tokenRemoved: true,
          remotelyRevoked: false,
          keychainEmpty: true,
          accountSessionsUrl: "https://www.epicgames.com/account/personal",
        });
      }
      return Promise.reject(new Error(`Orden no simulada: ${command}`));
    });

    renderPanel();
    const epicCard = await screen.findByRole("article", { name: "Epic Games Store" });
    await user.click(within(epicCard).getByRole("button", { name: /Cerrar sesión/ }));

    // Borrado local confirmado y revocación remota no confirmada son dos hechos
    // distintos, y se cuentan por separado en vez de con un «listo».
    expect(await screen.findByText(/se ha comprobado que ya no queda nada/)).toBeVisible();
    expect(screen.getByText(/no ha podido invalidarlo también en Epic Games Store/)).toBeVisible();
  });

  it("distingue el emparejado automático del que decidió la persona usuaria", async () => {
    mockedApi.listExternalGames.mockResolvedValue({
      items: [
        externalGame({ externalId: "SinPareja", title: "Juego sin pareja" }),
        externalGame({
          externalId: "Automatico",
          title: "Juego automático",
          matchedAppId: 367520,
          matchedTitle: "Hollow Knight",
          matchConfidence: 0.99,
          matchSource: "automatic",
        }),
        externalGame({
          externalId: "Manual",
          title: "Juego corregido",
          matchedAppId: 22370,
          matchedTitle: "Fallout 3",
          matchConfidence: 1,
          matchSource: "manual",
        }),
      ],
      total: 3,
      limit: 60,
      offset: 0,
    });

    renderPanel();
    expect(await screen.findByText("Sin emparejar")).toBeVisible();
    expect(screen.getByText("Emparejado automático")).toBeVisible();
    expect(screen.getByText("Emparejado por ti")).toBeVisible();
    // Sólo el corregido a mano ofrece devolverlo al algoritmo.
    expect(screen.getAllByRole("button", { name: "Volver al automático" })).toHaveLength(1);
  });

  it("desemparejar guarda la decisión de la persona, no borra el juego", async () => {
    const user = userEvent.setup();
    const matched = externalGame({
      externalId: "Automatico",
      title: "Juego automático",
      matchedAppId: 367520,
      matchedTitle: "Hollow Knight",
      matchConfidence: 0.99,
      matchSource: "automatic",
    });
    mockedApi.listExternalGames.mockResolvedValue({
      items: [matched],
      total: 1,
      limit: 60,
      offset: 0,
    });
    mockedApi.setExternalGameMatch.mockResolvedValue({
      ...matched,
      matchedAppId: null,
      matchedTitle: null,
      matchConfidence: 1,
      matchSource: "manual",
    });

    renderPanel();
    await user.click(await screen.findByRole("button", { name: "Desemparejar" }));

    expect(mockedApi.setExternalGameMatch).toHaveBeenCalledWith("epic", "Automatico", null);
    expect(
      await screen.findByText(
        "«Juego automático» quedó marcado como juego distinto a los de tu biblioteca de Steam.",
      ),
    ).toBeVisible();
  });

  it("el emparejado manual sólo ofrece juegos de la biblioteca real", async () => {
    const user = userEvent.setup();
    mockedApi.listExternalGames.mockResolvedValue({
      items: [externalGame({ externalId: "SinPareja", title: "Juego sin pareja" })],
      total: 1,
      limit: 60,
      offset: 0,
    });
    mockedApi.listGames.mockResolvedValue({
      items: [{ appId: 22370, title: "Fallout 3" }],
      total: 1,
      limit: 8,
      offset: 0,
    });
    mockedApi.setExternalGameMatch.mockResolvedValue(
      externalGame({
        externalId: "SinPareja",
        title: "Juego sin pareja",
        matchedAppId: 22370,
        matchedTitle: "Fallout 3",
        matchConfidence: 1,
        matchSource: "manual",
      }),
    );

    renderPanel();
    await user.click(await screen.findByRole("button", { name: "Emparejar" }));
    // Sin texto suficiente no se propone nada: el emparejado lo decide la persona.
    expect(
      screen.getByText(
        "Vindexa no propone nada por su cuenta aquí: elige tú el juego de tu biblioteca.",
      ),
    ).toBeVisible();

    await user.type(
      screen.getByRole("textbox", { name: "Buscar el juego de Steam con el que emparejar" }),
      "fallout",
    );
    await user.click(await screen.findByRole("button", { name: /Fallout 3/ }));

    expect(mockedApi.setExternalGameMatch).toHaveBeenCalledWith("epic", "SinPareja", 22370);
  });

  it("un juego de GOG sin ejecutable validado no promete lanzar, dice que abre Galaxy", async () => {
    mockedApi.listExternalGames.mockResolvedValue({
      items: [
        externalGame({
          store: "gog",
          externalId: "1207658924",
          title: "Juego de GOG",
          launchTarget: null,
        }),
      ],
      total: 1,
      limit: 60,
      offset: 0,
    });

    renderPanel();
    const action = await screen.findByRole("button", { name: /Abrir en GOG Galaxy/ });
    expect(action).toHaveAttribute(
      "title",
      "GOG no expone un lanzamiento directo y aquí no hay ningún ejecutable validado: se abrirá la ficha del juego dentro de Galaxy.",
    );
    expect(screen.queryByRole("button", { name: /Jugar/ })).toBeNull();
  });

  it("lanzar un juego externo delega en el cliente oficial de su tienda", async () => {
    const user = userEvent.setup();
    mockedApi.listExternalGames.mockResolvedValue({
      items: [externalGame({ externalId: "Instalado", title: "Juego instalado" })],
      total: 1,
      limit: 60,
      offset: 0,
    });
    mockedApi.launchExternalGame.mockResolvedValue(undefined);

    renderPanel();
    await user.click(await screen.findByRole("button", { name: /Jugar/ }));
    expect(mockedApi.launchExternalGame).toHaveBeenCalledWith("epic", "Instalado");
  });
});
