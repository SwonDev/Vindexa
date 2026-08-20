import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "./support/fixtures";

const viewports = [
  { name: "compact-960x700", width: 960, height: 700 },
  { name: "desktop-1440x900", width: 1440, height: 900 },
  { name: "ultrawide-1920x900", width: 1920, height: 900 },
] as const;

for (const viewport of viewports) {
  test(`la biblioteca no se corta en ${viewport.width}x${viewport.height}`, async ({
    app,
    page,
  }) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await app.goto();
    await app.waitForShell();
    await expect(app.gameButton("Nebula Frontier")).toBeVisible();
    await app.expectNoHorizontalOverflow();
    await expect(page).toHaveScreenshot(`${viewport.name}.png`, { fullPage: true });
  });
}

test("la vista ultracompacta mantiene varias filas legibles a 960x700", async ({ app, page }) => {
  await page.setViewportSize({ width: 960, height: 700 });
  await app.goto();
  await app.waitForShell();
  const compactView = page.getByRole("radio", { name: "Vista ultracompacta" });
  await compactView.click();
  // El conmutador es un grupo de radios nativo: el estado se marca con
  // `aria-checked`, no con `aria-pressed`, que es de los botones de dos estados.
  await expect(compactView).toBeChecked();
  await expect(page.locator(".game-browser--compact")).toBeVisible();
  await expect(page.locator(".game-row")).toHaveCount(3);
  await app.expectNoHorizontalOverflow();
  await expect(page).toHaveScreenshot("ultracompact-960x700.png", { fullPage: true });
});

/**
 * El catálogo de Steam Family a 960×700.
 *
 * Dejó de ser una pantalla aparte con su propia barra y su propia rejilla: es
 * un ámbito más de la biblioteca. Esta prueba se quedó comprobando aquella
 * pantalla —`.family-catalog-browser`, «Vista familiar ultracompacta», una fila
 * por juego— y fallaba entera desde entonces. Lo que sigue teniendo sentido
 * comprobar es lo de siempre: que en la ventana pequeña quepa, que la barra de
 * herramientas no se pierda al desplazarse y que no haya barreras de acceso.
 */
test("Steam Family cabe en 960x700 sin perder los controles", async ({ app, page }) => {
  await page.setViewportSize({ width: 960, height: 700 });
  await app.goto();
  await app.waitForShell();
  await page.getByRole("button", { name: /Steam Family/ }).click();

  await expect(page.getByRole("button", { name: /^Cuenta de Steam vinculada/ })).toBeVisible();
  await expect(page.getByRole("combobox", { name: "Ordenar biblioteca" })).toBeVisible();
  await expect(page.getByRole("combobox", { name: "Agrupar biblioteca" })).toBeVisible();
  await expect(page.locator(".game-browser")).toBeVisible();
  await app.expectNoHorizontalOverflow();

  // La barra de herramientas es fija: desplazarse no puede esconderla bajo el
  // listado, que es justo lo que comprobaba la versión anterior de esta prueba.
  await page.locator(".game-browser").evaluate((node) => {
    node.scrollTop = 400;
    node.dispatchEvent(new Event("scroll"));
  });
  await page.evaluate(async () => {
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
    );
  });
  const toolbarOnTop = await page.locator(".library-toolbar").evaluate((element) => {
    const bounds = element.getBoundingClientRect();
    const hit = document.elementFromPoint(bounds.left + 8, bounds.top + bounds.height / 2);
    return hit === element || element.contains(hit);
  });
  expect(toolbarOnTop).toBe(true);

  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
    .analyze();
  const blocking = results.violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(blocking).toEqual([]);
  await expect(page).toHaveScreenshot("family-grid-960x700.png", { fullPage: true });
});

test("la ficha desactiva por completo el parallax con movimiento reducido", async ({
  app,
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await app.goto();
  await app.openGame("Nebula Frontier");
  const sheet = page.locator(".game-detail-sheet");
  await sheet.evaluate((element) => {
    element.scrollTop = 420;
    element.dispatchEvent(new Event("scroll"));
  });
  await expect(page.locator(".detail-hero__media")).toHaveCSS("transform", "none");
  await expect(page.locator(".detail-hero__media")).toHaveCSS("opacity", "1");
});

test("la biblioteca no contiene violaciones axe serias o críticas", async ({ app, page }) => {
  await app.goto();
  await app.waitForShell();
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
    .analyze();
  const blocking = results.violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(blocking).toEqual([]);
});
