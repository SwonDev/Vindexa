import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { GameDetailSheet } from "@/features/library/GameDetailSheet";
import { api } from "@/lib/tauri";
import type { DlcSummary, GameDetail, PriorityExplanation } from "@/lib/types";
import "@/index.css";

vi.mock("@/components/common/Artwork", () => ({
  Artwork: ({ title, kind }: { title: string; kind: string }) => (
    <div className="artwork" data-kind={kind} role="img" aria-label={`Arte de ${title}`} />
  ),
}));
vi.mock("@/lib/tauri", () => ({
  api: {
    gameDetail: vi.fn(),
    refreshGameMetadata: vi.fn(),
    refreshGameAchievements: vi.fn(),
    updateGame: vi.fn(),
    setGameCollections: vi.fn(),
    listTags: vi.fn(),
    saveTag: vi.fn(),
    deleteTag: vi.fn(),
    setGameTags: vi.fn(),
    listGameSessions: vi.fn(),
    saveGameSession: vi.fn(),
    deleteGameSession: vi.fn(),
    savePersonalDates: vi.fn(),
    launchGame: vi.fn(),
    installGame: vi.fn(),
    uninstallGame: vi.fn(),
    revealInstallation: vi.fn(),
    openStore: vi.fn(),
    listGameDlc: vi.fn(),
    refreshGameDlc: vi.fn(),
    setDlcOwned: vi.fn(),
    setDlcHidden: vi.fn(),
    setDlcInstalled: vi.fn(),
    dlcSummary: vi.fn(),
    explainPriority: vi.fn(),
    setPriorityLock: vi.fn(),
    recomputePriorities: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "Error inesperado",
}));

const mockedApi = api as unknown as Record<keyof typeof api, ReturnType<typeof vi.fn>>;

const baseDetail = {
  appId: 620,
  title: "Portal 2",
  coverUrl: "https://example.test/cover.jpg",
  headerUrl: "https://example.test/header.jpg",
  heroUrl: "https://example.test/hero.jpg",
  playtimeMinutes: 800,
  playtimeRecentMinutes: 20,
  lastPlayedAt: "2026-08-14T10:00:00Z",
  releaseDate: "2011-04-19",
  isEarlyAccess: false,
  steamDeckStatus: "Verificado",
  achievementsTotal: 51,
  installed: false,
  statusId: "playing",
  statusName: "Jugando",
  statusColor: "#5CAAC1",
  progress: 55,
  priority: 4,
  pinned: false,
  tracking: false,
  manualPosition: 0,
  drmState: "unknown",
  shortDescription: "Una aventura cooperativa entre portales.",
  developer: "Valve",
  genres: ["Acción"],
  categories: [],
  metadataStatus: "success",
  metadataFetchedAt: "2026-08-14T10:00:00Z",
  achievementsStatus: "pending",
  isFree: false,
  ownershipSource: "owned",
  familyAvailability: "not_applicable",
  collectionIds: [],
  tags: [],
  sessions: [],
  activity: [],
  drm: { state: "unknown", evidence: [] },
  screenshots: [],
  movies: [],
} satisfies GameDetail;

/** Metadatos enriquecidos tal y como los serializa `db::rich_metadata`. */
const enriched = {
  ...baseDetail,
  detailedDescription: {
    blocks: [
      { kind: "heading", level: 2, text: "Cooperativo" },
      { kind: "paragraph", text: "Dos cámaras de pruebas simultáneas." },
      {
        kind: "list",
        ordered: false,
        items: ["Puzles con portales", "Gel de propulsión", "Editor de niveles"],
      },
      { kind: "paragraph", text: "<script>alert(1)</script> no debe ejecutarse." },
    ],
  },
  supportedLanguages: "Español, Inglés*, Francés",
  websiteUrl: "https://www.thinkwithportals.com/",
  metacriticScore: 95,
  metacriticUrl: "https://www.metacritic.com/game/portal-2",
  requiredAge: 12,
  controllerSupport: "full",
  drmState: "drm_free",
  drmNotice: "Este producto no incluye protección anticopia de terceros.",
  drmEvidence: [{ source: "drmNotice", match: "sin DRM" }],
  media: [
    {
      mediaId: "screenshot:1",
      kind: "screenshot",
      thumbnailUrl: "https://shared.steamstatic.com/apps/620/ss1.600x338.jpg",
      fullUrl: "https://shared.steamstatic.com/apps/620/ss1.1920x1080.jpg",
      position: 0,
    },
    {
      mediaId: "movie:2",
      kind: "movie",
      thumbnailUrl: "https://shared.steamstatic.com/apps/620/movie2.jpg",
      fullUrl: "https://media.steampowered.com/apps/620/movie2.mp4",
      position: 0,
    },
  ],
} as unknown as GameDetail;

function renderSheet() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <GameDetailSheet
          appId={620}
          open
          onOpenChange={vi.fn()}
          statuses={[
            {
              id: "playing",
              name: "Jugando",
              color: "#5CAAC1",
              position: 0,
              builtIn: true,
              gameCount: 1,
            },
          ]}
          collections={[]}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

/** Los paneles nuevos de la ficha piden estos datos nada más abrirse. */
const emptyDlcSummary = {
  appId: 620,
  total: 0,
  owned: 0,
  installed: 0,
  hidden: 0,
  free: 0,
  pending: 0,
  pendingCounted: 0,
  pendingUnknownPrice: 0,
  pendingOtherCurrency: 0,
} as DlcSummary;

const neutralPriority = {
  appId: 620,
  title: "Portal 2",
  score: 40,
  effectiveScore: 40,
  derivedPriority: 2,
  manualPriority: 2,
  locked: false,
  reason: "Sin señales que lo muevan por ahora.",
  computedAt: "2026-08-18T09:00:00Z",
  manualOverride: null,
  signals: [],
} as PriorityExplanation;

function serveDetail(value: GameDetail) {
  mockedApi.gameDetail.mockResolvedValue(value);
  mockedApi.refreshGameMetadata.mockResolvedValue(value);
}

describe("estructura de la descripción de la ficha", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    serveDetail(enriched);
    mockedApi.listTags.mockResolvedValue([]);
    mockedApi.listGameSessions.mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
    mockedApi.updateGame.mockResolvedValue(enriched);
    mockedApi.openStore.mockResolvedValue(undefined);
    mockedApi.dlcSummary.mockResolvedValue(emptyDlcSummary);
    mockedApi.listGameDlc.mockResolvedValue([]);
    mockedApi.explainPriority.mockResolvedValue(neutralPriority);
    mockedApi.setPriorityLock.mockResolvedValue(undefined);
  });

  it("jerarquiza resumen destacado, descripción larga, especificaciones y medios en ese orden", async () => {
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });

    const about = document.querySelector(".detail-about") as HTMLElement;
    const specs = document.querySelector(".detail-specs") as HTMLElement;
    const media = document.querySelector(".detail-media") as HTMLElement;
    expect(about).toBeTruthy();
    expect(specs).toBeTruthy();
    expect(media).toBeTruthy();

    const lead = document.querySelector(".detail-about__lead") as HTMLElement;
    expect(lead).toHaveTextContent("Una aventura cooperativa entre portales.");
    expect(about.compareDocumentPosition(specs) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(specs.compareDocumentPosition(media) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  /**
   * Un juego de otra tienda no está esperando a Steam.
   *
   * La cola de metadatos excluye a propósito lo que viene de Epic, GOG o
   * itch.io: no existe en Steam y pedir su ficha allí no devolvería nada. Pero
   * la ficha decía «Cargando la descripción desde Steam…» con un girador
   * eterno, en trescientos dieciocho juegos, porque su estado se queda en
   * «pending» para siempre. Una espera que no acaba nunca es peor que decir que
   * no hay nada que esperar.
   */
  it("no finge que está cargando la ficha de un juego que no es de Steam", async () => {
    const { detailedDescription: _sinDescripcion, ...sinTexto } = enriched;
    serveDetail({
      ...sinTexto,
      shortDescription: "",
      metadataStatus: "pending",
      externalStore: "gog",
    });
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });

    expect(screen.queryByText(/Cargando la descripción desde Steam/)).toBeNull();
    expect(screen.getByText(/Este juego viene de GOG/)).toBeVisible();
  });

  /**
   * Y tampoco se le echa la culpa a Steam.
   *
   * Abrir la ficha de un juego de Epic le pedía a Steam el AppID que Vindexa se
   * había inventado para él. Steam contestaba que no existe, el juego quedaba
   * marcado «Ficha no publicada» y se ofrecía reintentarlo cada día: una
   * etiqueta que culpa a Steam de no publicar algo que nunca fue suyo, y un
   * botón que repite una petición imposible.
   */
  /**
   * Una celda que sólo puede decir «Sin datos» ocupa una columna para nada.
   *
   * Los logros y la valoración de Steam Deck los publica Steam. En un juego de
   * Epic no hay ni dato ni manera de conseguirlo, y «En disco» no significa
   * nada en uno que no está instalado: tres de las seis celdas del resumen no
   * decían nada. Es la misma regla que ya sigue la vista rápida de los regalos
   * de Epic.
   */
  it("el resumen no enseña celdas que sólo pueden decir «Sin datos»", async () => {
    serveDetail({
      ...enriched,
      externalStore: "epic",
      installed: false,
      achievementsStatus: "pending",
      steamDeckStatus: "unknown",
    });
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });

    const resumen = document.querySelector(".detail-metrics") as HTMLElement;
    expect(resumen).toBeTruthy();
    expect(within(resumen).queryByText("Logros")).toBeNull();
    expect(within(resumen).queryByText("Steam Deck")).toBeNull();
    expect(within(resumen).queryByText("En disco")).toBeNull();
    // Lo que sí se sabe sigue estando.
    expect(within(resumen).getByText("Tiempo de juego")).toBeVisible();
    expect(within(resumen).getByText("Última sesión")).toBeVisible();
  });

  it("no marca «ficha no publicada» en un juego que no es de Steam", async () => {
    const { detailedDescription: _sinDescripcion, ...sinTexto } = enriched;
    serveDetail({
      ...sinTexto,
      shortDescription: "",
      metadataStatus: "unavailable",
      externalStore: "epic",
    });
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });

    expect(screen.queryByText("Ficha no publicada")).toBeNull();
    expect(screen.queryByRole("button", { name: /Reintentar ficha/ })).toBeNull();
    expect(screen.getByText(/Este juego viene de Epic Games Store/)).toBeVisible();
  });

  /**
   * Y mil quinientos ochenta y tres más, por el mismo motivo.
   *
   * El catálogo de Steam Family tampoco se pregunta a Steam mientras no conste
   * que se puede jugar. Es la misma espera eterna que la de las otras tiendas,
   * en cinco veces más juegos.
   */
  it("no finge que está cargando la ficha de un juego familiar sin confirmar", async () => {
    const { detailedDescription: _sinDescripcion, ...sinTexto } = enriched;
    serveDetail({
      ...sinTexto,
      shortDescription: "",
      metadataStatus: "pending",
      ownershipSource: "family_shared",
      familyAvailability: "unknown",
    });
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });

    expect(screen.queryByText(/Cargando la descripción desde Steam/)).toBeNull();
    expect(screen.getByText(/catálogo de Steam Family/)).toBeVisible();
  });

  it("maqueta los bloques seguros como encabezados, párrafos y listas reales sin inyectar HTML", async () => {
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });

    const prose = document.getElementById("detail-description") as HTMLElement;
    expect(within(prose).getByRole("heading", { name: "Cooperativo", level: 4 })).toBeVisible();
    expect(prose.querySelectorAll("ul li")).toHaveLength(3);
    expect(within(prose).getByText("Gel de propulsión")).toBeVisible();
    // El texto peligroso llega como contenido, nunca como marcado ejecutable.
    expect(prose.querySelector("script")).toBeNull();
    expect(prose.textContent).toContain("<script>alert(1)</script> no debe ejecutarse.");
  });

  it("pliega la prosa larga con un control accesible y sin reservar espacio cuando es corta", async () => {
    const user = userEvent.setup();
    const longDetail = {
      ...enriched,
      detailedDescription: {
        blocks: Array.from({ length: 12 }, (_, index) => ({
          kind: "paragraph",
          text: `Bloque ${index} con texto suficientemente extenso para desbordar. `.repeat(4),
        })),
      },
    } as unknown as GameDetail;
    serveDetail(longDetail);
    renderSheet();

    const toggle = await screen.findByRole("button", { name: "Mostrar más" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle).toHaveAttribute("aria-controls", "detail-description");
    expect(document.getElementById("detail-description")).toHaveAttribute("data-collapsed", "true");

    await user.click(toggle);
    expect(screen.getByRole("button", { name: "Mostrar menos" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(document.getElementById("detail-description")).not.toHaveAttribute("data-collapsed");
  });

  it("no dibuja el plegado ni la sección de medios cuando no hay contenido que plegar", async () => {
    serveDetail({ ...baseDetail } as GameDetail);
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });

    expect(screen.queryByRole("button", { name: "Mostrar más" })).not.toBeInTheDocument();
    expect(document.querySelector(".detail-media")).toBeNull();
    expect(document.querySelector(".detail-specs")).toBeNull();
  });

  it("muestra idiomas, edad, mando y Metacritic sólo cuando la tienda los publica", async () => {
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });

    const specs = document.querySelector(".detail-specs") as HTMLElement;
    expect(within(specs).getByText("Idiomas")).toBeVisible();
    expect(within(specs).getByText("Español")).toBeVisible();
    expect(within(specs).getByText("Inglés")).toBeVisible();
    expect(within(specs).getByText("12+")).toBeVisible();
    expect(within(specs).getByText("Compatible completo")).toBeVisible();
    expect(within(specs).getByText("95")).toBeVisible();
    expect(within(specs).getByText("thinkwithportals.com")).toBeVisible();
  });

  it("omite Metacritic, edad y mando cuando la tienda no los declara", async () => {
    // La tienda no declara ni Metacritic ni el soporte de mando: las claves no
    // llegan. Escribirles `undefined` sería decir que llegan vacías.
    const { metacriticScore: _sinNota, controllerSupport: _sinMando, ...sinExtras } = enriched;
    serveDetail({ ...sinExtras, requiredAge: 0 });
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });

    const specs = await screen.findByText("Idiomas");
    const list = specs.closest(".detail-specs") as HTMLElement;
    expect(within(list).queryByText("Metacritic")).not.toBeInTheDocument();
    expect(within(list).queryByText("Edad recomendada")).not.toBeInTheDocument();
    expect(within(list).queryByText("Mando")).not.toBeInTheDocument();
    // La protección y el sitio oficial siguen presentes: sólo desaparece lo ausente.
    expect(within(list).getByText("Protección")).toBeVisible();
  });

  it("coloca la marca sin DRM en la ficha y nunca sobre la carátula", async () => {
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });

    const badge = screen.getByRole("button", { name: /Sin DRM/ });
    expect(badge).toHaveAttribute("data-state", "drm_free");
    expect(badge.closest(".detail-specs")).toBeTruthy();
    expect(badge.closest(".detail-hero")).toBeNull();

    const hero = document.querySelector(".detail-hero") as HTMLElement;
    expect(hero.textContent).not.toMatch(/DRM/i);
    const artwork = document.querySelector(".detail-hero__media .artwork") as HTMLElement;
    expect(artwork.textContent).not.toMatch(/DRM/i);
  });

  /**
   * La evidencia es lo que hace comprobable la marca.
   *
   * Antes vivía dentro de un emergente, concatenada y con el **nombre interno
   * del campo** de la respuesta de la tienda: «extUserAccountNotice → Rockstar
   * Games». Un dato que no se entiende no se puede comprobar, y una marca de
   * DRM sin evidencia comprobable es una insignia.
   */
  it("enseña de dónde sale la marca de DRM, en palabras", async () => {
    serveDetail({
      ...enriched,
      drmState: "third_party_drm",
      drmEvidence: [
        { source: "drmNotice", match: "Denuvo Anti-Tamper" },
        { source: "extUserAccountNotice", match: "Rockstar Games" },
      ],
    } as unknown as GameDetail);
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });

    const evidencia = document.querySelector(".detail-drm__evidence") as HTMLElement;
    expect(evidencia).not.toBeNull();
    expect(within(evidencia).getByText("Aviso de DRM de la ficha")).toBeVisible();
    expect(within(evidencia).getByText("Denuvo Anti-Tamper")).toBeVisible();
    expect(within(evidencia).getByText("Cuenta externa que pide la ficha")).toBeVisible();
    expect(within(evidencia).getByText("Rockstar Games")).toBeVisible();
    // Y el nombre interno del campo no aparece en ninguna parte.
    expect(evidencia.textContent).not.toContain("extUserAccountNotice");
  });

  it("publica capturas con texto alternativo y deriva el vídeo a la tienda integrada", async () => {
    const user = userEvent.setup();
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });

    expect(screen.getByAltText("Captura 1 de Portal 2")).toBeVisible();
    const video = screen.getByRole("button", { name: /Vídeo 2 de Portal 2/ });
    await user.click(video);
    await waitFor(() => expect(mockedApi.openStore).toHaveBeenCalledWith(620));
  });

  it("omite las filas de información sin dato en lugar de rellenarlas con un guion", async () => {
    const user = userEvent.setup();
    serveDetail({ ...baseDetail } as GameDetail);
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });
    await user.click(screen.getByRole("tab", { name: "Información" }));

    const info = document.querySelector(".detail-info") as HTMLElement;
    expect(within(info).getByText("Desarrollador")).toBeVisible();
    expect(within(info).queryByText("Editor")).not.toBeInTheDocument();
    expect(within(info).queryByText("Instalación")).not.toBeInTheDocument();
    expect(within(info).queryByText("Categorías")).not.toBeInTheDocument();
    expect(info.textContent).not.toContain("—");
  });
});

describe("tratamiento del banner", () => {
  const css = readFileSync(resolve(process.cwd(), "src/features/library/game-detail.css"), "utf8");

  it("cierra el degradado sobre el color de la barra y no contra var(--card) opaco", () => {
    const scrim = css.slice(css.indexOf(".detail-hero .detail-hero__scrim"));
    const rule = scrim.slice(0, scrim.indexOf("}"));
    expect(rule).toContain("var(--v-surface-raised) 100%");
    // El corte duro contra `var(--card)` era el origen de la banda visible y de
    // la mitad inferior del banner completamente borrada.
    expect(rule).not.toContain("var(--card)");
    // El degradado sólo empieza a actuar en el tercio inferior.
    expect(rule).toContain("rgb(10 14 19 / 0%) 44%");
  });

  /**
   * El banner pide el arte de biblioteca, no el fondo de la página de tienda.
   *
   * `heroUrl` es el fondo que Steam pinta detrás del texto de su página: casi
   * siempre un degradado oscuro. Pasárselo al caché de arte no sólo elegía esa
   * imagen apagada, sino que **impedía** encontrar la buena: el caché ordena
   * los candidatos por calidad y sólo prueba los que mejoran lo que la interfaz
   * pidió, y una URL que no es ninguno de los peldaños conocidos entra como la
   * mejor posible y corta la búsqueda. Sin fuente explícita recorre la escalera
   * entera y encuentra `library_hero`, que es el arte a todo color.
   *
   * Medido sobre la biblioteca real: de diez juegos que se veían apagados,
   * nueve tienen `library_hero` publicado.
   */
  it("no le pasa al caché el fondo de la página de tienda como banner", async () => {
    const fuente = readFileSync(
      resolve(process.cwd(), "src/features/library/GameDetailSheet.tsx"),
      "utf8",
    );
    const banner = fuente.slice(fuente.indexOf("detail-hero__media"));
    const artwork = banner.slice(banner.indexOf("<Artwork"), banner.indexOf("/>") + 2);
    // El orden importa: `libraryHeroUrl` antes que `heroUrl`. Al revés, el
    // caché recibe una URL que no reconoce, la toma por la mejor posible y no
    // busca el arte de biblioteca.
    const posicionBiblioteca = artwork.indexOf("detail.libraryHeroUrl");
    const posicionFondo = artwork.indexOf("detail.heroUrl");
    expect(posicionBiblioteca).toBeGreaterThanOrEqual(0);
    expect(posicionFondo).toBeGreaterThan(posicionBiblioteca);
  });

  it("realza el color del arte en vez de desaturarlo", () => {
    const media = css.slice(css.indexOf(".detail-hero .detail-hero__media .artwork"));
    const rule = media.slice(0, media.indexOf("}"));
    expect(rule).toContain("saturate(1.04)");
    expect(rule).not.toMatch(/grayscale|saturate\(0/);
  });
});
