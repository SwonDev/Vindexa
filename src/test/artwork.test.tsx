import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Artwork } from "@/components/common/Artwork";

const mocks = vi.hoisted(() => ({
  cacheGameArt: vi.fn(),
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
}));

vi.mock("@/lib/tauri", () => ({
  api: { cacheGameArt: mocks.cacheGameArt },
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: mocks.convertFileSrc,
}));

interface ObservedArtwork {
  callback: IntersectionObserverCallback;
  element?: Element;
  disconnected: boolean;
}

let observedArtwork: ObservedArtwork[] = [];

class IntersectionObserverMock {
  readonly root = null;
  readonly rootMargin = "240px 0px";
  readonly thresholds = [0];
  private readonly record: ObservedArtwork;

  constructor(callback: IntersectionObserverCallback) {
    this.record = { callback, disconnected: false };
    observedArtwork.push(this.record);
  }

  observe(element: Element) {
    this.record.element = element;
  }

  unobserve() {}

  disconnect() {
    this.record.disconnected = true;
  }

  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }
}

function reveal(records = observedArtwork) {
  for (const record of records) {
    if (!record.element || record.disconnected) continue;
    record.callback(
      [
        {
          target: record.element,
          isIntersecting: true,
          intersectionRatio: 1,
          boundingClientRect: record.element.getBoundingClientRect(),
          intersectionRect: record.element.getBoundingClientRect(),
          rootBounds: null,
          time: performance.now(),
        },
      ],
      {} as IntersectionObserver,
    );
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

describe("artwork local-first", () => {
  beforeEach(() => {
    observedArtwork = [];
    mocks.cacheGameArt.mockReset();
    mocks.convertFileSrc.mockClear();
    Object.defineProperty(window, "IntersectionObserver", {
      configurable: true,
      writable: true,
      value: IntersectionObserverMock,
    });
  });

  it("no pinta la URL remota y sustituye el fallback por el asset local", async () => {
    const request = deferred<{ localPath: string }>();
    mocks.cacheGameArt.mockReturnValueOnce(request.promise);
    const { container } = render(
      <Artwork
        appId={910_001}
        src="https://shared.steamstatic.com/store_item_assets/steam/apps/910001/cover.jpg"
        title="Nebula Forge"
      />,
    );

    expect(screen.getByRole("img", { name: "Carátula de Nebula Forge" })).toHaveTextContent("NF");
    expect(container.querySelector("img")).toBeNull();
    expect(mocks.cacheGameArt).not.toHaveBeenCalled();

    act(() => reveal());
    expect(mocks.cacheGameArt).toHaveBeenCalledWith(910_001, "cover");
    await act(async () => request.resolve({ localPath: "/cache/910001/cover.jpg" }));

    const image = await screen.findByRole("img", { name: "Carátula de Nebula Forge" });
    expect(image).toHaveAttribute("src", "asset:///cache/910001/cover.jpg");
    expect(image).toHaveAttribute("loading", "lazy");
    expect(container.innerHTML).not.toContain("https://shared.steamstatic.com");
  });

  it("deduplica montajes concurrentes del mismo juego y variante", async () => {
    const request = deferred<{ localPath: string }>();
    mocks.cacheGameArt.mockReturnValue(request.promise);
    render(
      <>
        <Artwork appId={910_002} src="https://steam.test/one.jpg" title="Aurora" />
        <Artwork appId={910_002} src="https://steam.test/one.jpg" title="Aurora" />
      </>,
    );

    act(() => reveal());
    expect(mocks.cacheGameArt).toHaveBeenCalledTimes(1);
    await act(async () => request.resolve({ localPath: "/cache/910002/cover.jpg" }));
    expect(await screen.findAllByRole("img", { name: "Carátula de Aurora" })).toHaveLength(2);
  });

  it("memoriza fallos brevemente y evita una tormenta de reintentos", async () => {
    mocks.cacheGameArt.mockRejectedValueOnce(new Error("no disponible"));
    const first = render(
      <Artwork appId={910_003} src="https://steam.test/missing.jpg" title="Sin arte" />,
    );
    act(() => reveal());
    await waitFor(() => expect(mocks.cacheGameArt).toHaveBeenCalledTimes(1));
    first.unmount();

    render(<Artwork appId={910_003} src="https://steam.test/missing.jpg" title="Sin arte" />);
    act(() => reveal());
    await waitFor(() =>
      expect(screen.getByRole("img", { name: "Carátula de Sin arte" })).toBeVisible(),
    );
    expect(mocks.cacheGameArt).toHaveBeenCalledTimes(1);
  });

  it("prioriza cabeceras visibles sin esperar al observador", async () => {
    mocks.cacheGameArt.mockResolvedValueOnce({ localPath: "/cache/910004/header.jpg" });
    render(
      <Artwork
        appId={910_004}
        src="https://steam.test/header.jpg"
        title="Titan Circuit"
        kind="header"
      />,
    );

    expect(mocks.cacheGameArt).toHaveBeenCalledWith(910_004, "header");
    await waitFor(() =>
      expect(screen.getByRole("img", { name: "Cabecera de Titan Circuit" })).toHaveAttribute(
        "loading",
        "eager",
      ),
    );
    expect(screen.getByRole("img", { name: "Cabecera de Titan Circuit" })).toHaveAttribute(
      "fetchpriority",
      "high",
    );
    expect(observedArtwork).toHaveLength(0);
  });

  it("resuelve el hero de Store localmente y lo marca como recurso prioritario", async () => {
    mocks.cacheGameArt.mockResolvedValueOnce({ localPath: "/cache/910006/hero.webp" });
    render(
      <Artwork
        appId={910_006}
        src="https://store.akamai.steamstatic.com/images/storepagebackground/app/910006?t=1"
        title="Portal Vector"
        kind="hero"
      />,
    );

    expect(mocks.cacheGameArt).toHaveBeenCalledWith(910_006, "hero");
    await waitFor(() =>
      expect(screen.getByRole("img", { name: "Arte principal de Portal Vector" })).toHaveAttribute(
        "src",
        "asset:///cache/910006/hero.webp",
      ),
    );
    expect(screen.getByRole("img", { name: "Arte principal de Portal Vector" })).toHaveAttribute(
      "fetchpriority",
      "high",
    );
  });

  it("vuelve al fallback accesible si el archivo local está corrupto", async () => {
    mocks.cacheGameArt.mockResolvedValueOnce({ localPath: "/cache/910005/cover.jpg" });
    render(
      <Artwork
        appId={910_005}
        src="https://steam.test/corrupt.jpg"
        title="Broken Signal"
        priority
      />,
    );
    await waitFor(() =>
      expect(screen.getByRole("img", { name: "Carátula de Broken Signal" }).tagName).toBe("IMG"),
    );
    const image = screen.getByRole("img", { name: "Carátula de Broken Signal" });
    fireEvent.error(image);
    expect(screen.getByRole("img", { name: "Carátula de Broken Signal" })).toHaveTextContent("BS");
  });
});
