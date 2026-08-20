import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { GameVideoPanel } from "@/features/wishlist/GameVideoPanel";
import { api } from "@/lib/tauri";
import type { GameVideo, VideoKind } from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    listGameVideos: vi.fn(),
    saveGameVideo: vi.fn(),
    deleteGameVideo: vi.fn(),
    reorderGameVideos: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

vi.mock("@/components/common/Artwork", () => ({
  Artwork: ({ title }: { title: string }) => <div data-artwork={title} aria-hidden="true" />,
  // La precarga es una mejora de tiempos: en pruebas basta con que exista.
  prefetchArtwork: () => undefined,
}));

const mockedApi = api as unknown as { [Key in keyof typeof api]: ReturnType<typeof vi.fn> };

const APP_ID = 1030300;

function video(
  videoId: string,
  kind: VideoKind,
  position: number,
  extra: Partial<GameVideo> = {},
): GameVideo {
  return {
    appId: APP_ID,
    videoId,
    provider: "youtube",
    kind,
    title: `Vídeo ${videoId}`,
    channel: "Canal de referencia",
    source: "manual",
    position,
    createdAt: "2026-08-01T10:00:00Z",
    // La construye Rust tras validar el identificador. La prueba la trata como
    // un dato opaco, exactamente igual que la pantalla.
    embedUrl: `https://www.youtube-nocookie.com/embed/${videoId}`,
    ...extra,
  };
}

function renderPanel() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <GameVideoPanel
          appId={APP_ID}
          title="Hollow Knight: Silksong"
          headerUrl="https://cdn.example.invalid/header.jpg"
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

/**
 * Despliega el formulario de alta.
 *
 * Sus cuatro campos ocupaban la columna del panel siempre, aunque no hubiera
 * ni un vídeo guardado. Ahora se piden, así que las pruebas que escriben en
 * ellos tienen que abrirlo primero, igual que quien lo usa.
 */
async function abrirFormulario(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: /Añadir vídeo/ }));
}

describe("vídeos por juego · alta y validación", () => {
  beforeEach(() => {
    mockedApi.listGameVideos.mockResolvedValue([]);
    mockedApi.saveGameVideo.mockResolvedValue(video("abc12345678", "gameplay", 0));
    mockedApi.deleteGameVideo.mockResolvedValue(undefined);
    mockedApi.reorderGameVideos.mockResolvedValue(undefined);
  });

  it("no ocupa la columna con un formulario que nadie ha pedido", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText(/Todavía no hay ningún vídeo/);

    // Cuatro campos y un botón en una columna estrecha, para algo que se usa
    // una vez cada muchos juegos: de entrada no están.
    expect(screen.queryByLabelText("URL o identificador")).toBeNull();
    expect(screen.queryByLabelText("Título (opcional)")).toBeNull();

    const abrir = screen.getByRole("button", { name: /Añadir vídeo/ });
    expect(abrir).toHaveAttribute("aria-expanded", "false");
    await user.click(abrir);

    expect(screen.getByLabelText("URL o identificador")).toBeVisible();
    expect(screen.getByRole("button", { name: /Cancelar/ })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });

  it("manda al backend la URL tal cual: el identificador lo extrae y valida Rust", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText(/Todavía no hay ningún vídeo/);
    await abrirFormulario(user);

    await user.type(
      screen.getByLabelText("URL o identificador"),
      "https://www.youtube.com/watch?v=abc12345678",
    );
    await user.click(screen.getByRole("button", { name: /Guardar vídeo/ }));

    await waitFor(() =>
      expect(mockedApi.saveGameVideo).toHaveBeenCalledWith({
        appId: APP_ID,
        video: "https://www.youtube.com/watch?v=abc12345678",
        kind: "gameplay",
      }),
    );
  });

  it("clasifica el vídeo con el tipo elegido y adjunta título y canal cuando se escriben", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText(/Todavía no hay ningún vídeo/);
    await abrirFormulario(user);

    await user.type(screen.getByLabelText("URL o identificador"), "abc12345678");
    await user.click(screen.getByRole("combobox", { name: "Tipo de vídeo" }));
    await user.click(await screen.findByRole("option", { name: "Análisis" }));
    await user.type(screen.getByLabelText("Título (opcional)"), "Análisis a fondo");
    await user.type(screen.getByLabelText("Canal (opcional)"), "Mandalore");
    await user.click(screen.getByRole("button", { name: /Guardar vídeo/ }));

    await waitFor(() =>
      expect(mockedApi.saveGameVideo).toHaveBeenCalledWith({
        appId: APP_ID,
        video: "abc12345678",
        kind: "review",
        title: "Análisis a fondo",
        channel: "Mandalore",
      }),
    );
  });

  it("no llama al backend con el campo vacío y lo dice en el sitio", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText(/Todavía no hay ningún vídeo/);
    await abrirFormulario(user);

    await user.click(screen.getByRole("button", { name: /Guardar vídeo/ }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Pega la URL del vídeo o su identificador.",
    );
    expect(mockedApi.saveGameVideo).not.toHaveBeenCalled();
  });

  it("enseña sin adornos el motivo por el que el backend rechaza un enlace", async () => {
    const user = userEvent.setup();
    mockedApi.saveGameVideo.mockRejectedValue(
      // Mensaje literal de `parse_youtube_video_id` en `src-tauri/src/db/wishlist.rs`.
      new Error(
        "Pega el enlace de un vídeo de YouTube (youtube.com/watch, youtu.be o /embed) o su identificador de 11 caracteres.",
      ),
    );
    renderPanel();
    await screen.findByText(/Todavía no hay ningún vídeo/);
    await abrirFormulario(user);

    await user.type(screen.getByLabelText("URL o identificador"), "https://example.invalid/x");
    await user.click(screen.getByRole("button", { name: /Guardar vídeo/ }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Pega el enlace de un vídeo de YouTube (youtube.com/watch, youtu.be o /embed) o su identificador de 11 caracteres.",
    );
  });
});

describe("vídeos por juego · reproducción bajo demanda", () => {
  beforeEach(() => {
    mockedApi.deleteGameVideo.mockResolvedValue(undefined);
    mockedApi.reorderGameVideos.mockResolvedValue(undefined);
    mockedApi.saveGameVideo.mockResolvedValue(video("abc12345678", "gameplay", 0));
  });

  it("no monta ningún marco hasta que se pulsa reproducir, y entonces usa la URL del backend", async () => {
    const user = userEvent.setup();
    mockedApi.listGameVideos.mockResolvedValue([video("abc12345678", "gameplay", 0)]);
    renderPanel();

    const play = await screen.findByRole("button", { name: "Reproducir Vídeo abc12345678" });
    expect(document.querySelector("iframe")).toBeNull();

    await user.click(play);

    const frame = document.querySelector("iframe");
    expect(frame).toBeInTheDocument();
    expect(frame).toHaveAttribute("src", "https://www.youtube-nocookie.com/embed/abc12345678");
    expect(frame).toHaveAttribute("title", "Reproductor de Vídeo abc12345678");
    expect(frame).toHaveAttribute("referrerpolicy", "no-referrer");
  });

  it("nunca pide una miniatura remota: el cartel es la portada del propio juego", async () => {
    mockedApi.listGameVideos.mockResolvedValue([
      video("abc12345678", "gameplay", 0, {
        thumbnailUrl: "https://i.ytimg.com/vi/abc12345678/hqdefault.jpg",
      }),
    ]);
    renderPanel();
    await screen.findByRole("button", { name: "Reproducir Vídeo abc12345678" });

    expect(document.querySelector('img[src*="ytimg"]')).toBeNull();
    expect(document.querySelector('[src*="i.ytimg.com"]')).toBeNull();
    expect(document.querySelector('[data-artwork="Hollow Knight: Silksong"]')).toBeInTheDocument();
  });

  it("un vídeo sin URL incrustable no ofrece un botón de reproducción que no funciona", async () => {
    mockedApi.listGameVideos.mockResolvedValue([
      { ...video("steam-1", "trailer", 0), provider: "steam" as const, embedUrl: undefined },
    ]);
    renderPanel();

    const poster = await screen.findByRole("button", {
      name: "Vídeo steam-1 no se puede incrustar: ábrelo en su plataforma",
    });
    expect(poster).toBeDisabled();
    expect(screen.getByText("Sin reproducción incrustada")).toBeVisible();
  });
});

describe("vídeos por juego · organización", () => {
  beforeEach(() => {
    mockedApi.deleteGameVideo.mockResolvedValue(undefined);
    mockedApi.reorderGameVideos.mockResolvedValue(undefined);
    mockedApi.saveGameVideo.mockResolvedValue(video("abc12345678", "gameplay", 0));
    mockedApi.listGameVideos.mockResolvedValue([
      video("game-1", "gameplay", 0),
      video("game-2", "gameplay", 1),
      video("rev-1", "review", 0),
    ]);
  });

  it("agrupa por tipo y reordena dentro del grupo, que es como reordena el backend", async () => {
    const user = userEvent.setup();
    renderPanel();

    const group = await screen.findByRole("region", { name: "Gameplay (2)" });
    expect(within(group).getAllByText(/^Vídeo game-/)).toHaveLength(2);

    await user.click(screen.getByRole("button", { name: "Bajar Vídeo game-1" }));

    await waitFor(() =>
      expect(mockedApi.reorderGameVideos).toHaveBeenCalledWith(APP_ID, "gameplay", [
        { provider: "youtube", videoId: "game-2" },
        { provider: "youtube", videoId: "game-1" },
      ]),
    );
  });

  it("filtra por tipo sin volver a pedir la lista al backend", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByRole("region", { name: "Gameplay (2)" });
    expect(mockedApi.listGameVideos).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("radio", { name: "Análisis" }));

    expect(await screen.findByRole("region", { name: "Análisis (1)" })).toBeVisible();
    expect(screen.queryByRole("region", { name: "Gameplay (2)" })).toBeNull();
    expect(mockedApi.listGameVideos).toHaveBeenCalledTimes(1);
  });

  it("un filtro sin resultados explica qué hacer en lugar de quedarse en blanco", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByRole("region", { name: "Gameplay (2)" });

    await user.click(screen.getByRole("radio", { name: "Guía" }));

    expect(await screen.findByText(/Ninguno de tipo «Guía»/)).toBeVisible();
  });

  it("quita un vídeo identificándolo por proveedor e identificador", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByRole("region", { name: "Gameplay (2)" });

    await user.click(screen.getByRole("button", { name: "Quitar Vídeo rev-1" }));

    await waitFor(() =>
      expect(mockedApi.deleteGameVideo).toHaveBeenCalledWith(APP_ID, "youtube", "rev-1"),
    );
  });
});

describe("vídeos por juego · estados límite", () => {
  it("el estado vacío dice para qué sirve guardar un vídeo aquí", async () => {
    mockedApi.listGameVideos.mockResolvedValue([]);
    renderPanel();

    expect(
      await screen.findByText(
        "Guarda aquí el vídeo que te convenció, para no volver a buscarlo cuando dudes si comprarlo.",
      ),
    ).toBeVisible();
  });

  it("cuando la lectura falla ofrece reintentar", async () => {
    mockedApi.listGameVideos.mockRejectedValue(new Error("SQLite no responde."));
    renderPanel();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("No se pudieron leer los vídeos");
    expect(within(alert).getByRole("button", { name: "Reintentar" })).toBeVisible();
  });
});
