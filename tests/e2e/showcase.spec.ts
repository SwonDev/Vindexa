import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "./support/fixtures";

/**
 * Vitrina visual: no compara nada, genera capturas de alta fidelidad con arte
 * oficial para que la revisión de calidad juzgue la aplicación real y no una
 * maqueta. Vive fuera de la batería de regresión porque necesita red.
 */
const OUT = join(process.cwd(), "artifacts", "showcase");
mkdirSync(OUT, { recursive: true });

test.use({ scenario: "showcase" });
test.describe.configure({ mode: "serial" });

async function settleArtwork(page: import("@playwright/test").Page) {
  await page.evaluate(() => document.fonts.ready);
  // Basta con que la inmensa mayoría del arte esté resuelto: alguna portada
  // puede no existir en la CDN y bloquear la captura indefinidamente.
  await page
    .waitForFunction(
      () => {
        const images = Array.from(document.querySelectorAll("img"));
        const visible = images.filter((image) => image.getBoundingClientRect().width > 0);
        if (!visible.length) return false;
        const settled = visible.filter((image) => image.complete).length;
        return settled / visible.length >= 0.85;
      },
      undefined,
      { timeout: 20_000 },
    )
    .catch(() => undefined);
  await page.waitForTimeout(600);
}

const viewports = [
  { name: "1440x900", width: 1440, height: 900 },
  { name: "1920x1080", width: 1920, height: 1080 },
] as const;

for (const viewport of viewports) {
  test(`vitrina · biblioteca en rejilla ${viewport.name}`, async ({ app, page }) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await app.goto();
    await app.waitForShell();
    await settleArtwork(page);
    await page.screenshot({ path: join(OUT, `biblioteca-rejilla-${viewport.name}.png`) });

    // Desplazamiento: comprueba el fundido de las carátulas bajo la barra.
    await page.locator(".game-browser").evaluate((node) => {
      node.scrollTop = 260;
      node.dispatchEvent(new Event("scroll"));
    });
    await page.waitForTimeout(350);
    await page.screenshot({ path: join(OUT, `biblioteca-rejilla-scroll-${viewport.name}.png`) });
  });
}

test("vitrina · biblioteca en lista y ultracompacta", async ({ app, page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await app.goto();
  await app.waitForShell();

  await page.getByRole("button", { name: "Vista de lista" }).click();
  await settleArtwork(page);
  await page.screenshot({ path: join(OUT, "biblioteca-lista-1440x900.png") });

  await page.getByRole("button", { name: "Vista ultracompacta" }).click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: join(OUT, "biblioteca-ultracompacta-1440x900.png") });
});

test("vitrina · ficha de juego", async ({ app, page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await app.goto();
  await app.waitForShell();
  await settleArtwork(page);

  await app.openGame("ELDEN RING");
  await page.waitForTimeout(900);
  await page.screenshot({ path: join(OUT, "ficha-elden-ring-1440x900.png") });

  // El cuerpo de la ficha se desplaza dentro del propio diálogo. Se busca el
  // contenedor real y se comprueba que de verdad se movió: una captura idéntica
  // a la anterior no prueba nada.
  const moved = await page.getByRole("dialog").evaluate((dialog) => {
    // El propio panel es el contenedor desplazable, así que entra en la búsqueda.
    const candidates = [dialog, ...dialog.querySelectorAll<HTMLElement>("*")] as HTMLElement[];
    const scroller = candidates.find(
      (node) => node.scrollHeight > node.clientHeight + 40 && node.clientHeight > 160,
    );
    if (!scroller) return 0;
    scroller.scrollTop = 640;
    return scroller.scrollTop;
  });
  expect(moved, "la ficha debe poder desplazarse").toBeGreaterThan(0);
  await page.waitForTimeout(500);
  await page.screenshot({ path: join(OUT, "ficha-elden-ring-scroll-1440x900.png") });
});

for (const section of ["Planificador", "Colecciones", "Seguimiento"] as const) {
  test(`vitrina · ${section}`, async ({ app, page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await app.goto();
    await app.waitForShell();
    await app.navigate(section);
    if (section === "Seguimiento") {
      // La pantalla pide su recomendación al abrir: se espera al resultado real,
      // no a un plazo fijo, o la vitrina capturaría el esqueleto de carga.
      await expect(page.locator(".decision-result--busy")).toBeHidden({ timeout: 10_000 });
    }
    await page.waitForTimeout(900);
    await settleArtwork(page).catch(() => undefined);
    const slug = section.toLowerCase();
    await page.screenshot({ path: join(OUT, `${slug}-1440x900.png`) });

    // Segunda captura tras desplazar, para detectar cortes y scroll roto.
    const scroller = page
      .locator(".discovery-scroll, .planner-scroll, .collections-scroll")
      .first();
    if (await scroller.count()) {
      await scroller.evaluate((node) => {
        node.scrollTop = node.scrollHeight;
      });
      await page.waitForTimeout(350);
      await page.screenshot({ path: join(OUT, `${slug}-fondo-1440x900.png`) });
    }
    await expect(page.locator(".app-shell")).toBeVisible();
  });
}

test("vitrina · bandeja de avisos", async ({ app, page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await app.goto();
  await app.waitForShell();
  await settleArtwork(page);

  await page.getByRole("button", { name: /^Avisos:/ }).click();
  await expect(page.getByText("Hades II ha salido de acceso anticipado")).toBeVisible();
  await page.waitForTimeout(400);
  await page.screenshot({ path: join(OUT, "avisos-1440x900.png") });
});

test("vitrina · biblioteca agrupada por estado", async ({ app, page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await app.goto();
  await app.waitForShell();

  await page.getByRole("button", { name: "Vista de lista" }).click();
  await page.getByRole("combobox", { name: "Agrupar biblioteca" }).click();
  await page.getByRole("option", { name: "Estado" }).click();
  await expect(page.locator(".library-group-header").first()).toBeVisible();
  await settleArtwork(page);
  await page.screenshot({ path: join(OUT, "biblioteca-agrupada-1440x900.png") });
});

test("vitrina · contenido adicional y prioridad", async ({ app, page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await app.goto();
  await app.waitForShell();
  await settleArtwork(page);

  await app.openGame("Portal 2");
  await page.waitForTimeout(700);
  // La explicación de prioridad encabeza «Plan personal», bajo la descripción.
  await page.getByRole("dialog").evaluate((dialog) => {
    const candidates = [dialog, ...dialog.querySelectorAll<HTMLElement>("*")] as HTMLElement[];
    const scroller = candidates.find(
      (node) => node.scrollHeight > node.clientHeight + 40 && node.clientHeight > 160,
    );
    if (scroller) scroller.scrollTop = 700;
  });
  await page.waitForTimeout(400);
  await page.screenshot({ path: join(OUT, "ficha-prioridad-1440x900.png") });

  await page.getByRole("tab", { name: /Contenido adicional/ }).click();
  await expect(page.getByText("Banda sonora original")).toBeVisible();
  await page.waitForTimeout(500);
  await page.screenshot({ path: join(OUT, "ficha-dlc-1440x900.png") });
});

test("vitrina · paleta de comandos", async ({ app, page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await app.goto();
  await app.waitForShell();
  await settleArtwork(page);

  await page.getByRole("button", { name: "Abrir la paleta de comandos" }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.waitForTimeout(400);
  await page.screenshot({ path: join(OUT, "paleta-1440x900.png") });

  await page.getByRole("dialog").getByRole("combobox").fill("elden");
  await page.waitForTimeout(400);
  await page.screenshot({ path: join(OUT, "paleta-busqueda-1440x900.png") });
});

test("vitrina · deseados", async ({ app, page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await app.goto();
  await app.waitForShell();
  await app.navigate("Deseados");
  await page.waitForTimeout(1_200);
  await settleArtwork(page).catch(() => undefined);
  await page.screenshot({ path: join(OUT, "deseados-1440x900.png") });
});
