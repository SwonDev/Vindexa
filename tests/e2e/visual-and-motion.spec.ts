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

test("Steam Family conserva controles y filas densas a 960x700", async ({ app, page }) => {
  await page.setViewportSize({ width: 960, height: 700 });
  await app.goto();
  await app.waitForShell();
  await page.getByRole("button", { name: /Steam Family/ }).click();

  await expect(page.getByRole("button", { name: /^Cuenta vinculada · / })).toBeVisible();
  // El recuento vive en la barra lateral desde que la de estado dejó de repetir
  // lo que ya se ve en otro sitio.
  await expect(page.getByRole("button", { name: /^Todos los juegos/ })).toContainText("3");
  await expect(page.getByRole("combobox", { name: "Filtrar catálogo familiar" })).toBeVisible();
  await expect(page.getByRole("combobox", { name: "Ordenar catálogo familiar" })).toBeVisible();
  await expect(page.locator(".family-catalog-browser--grid")).toBeVisible();
  const gridRows = page.locator(".family-catalog-browser--grid .virtual-grid-row");
  await expect(gridRows).toHaveCount(3);
  await page.evaluate(async () => {
    await document.fonts.ready;
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() =>
        requestAnimationFrame(() =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        ),
      ),
    );
  });
  const firstRow = await gridRows.nth(0).boundingBox();
  const secondRow = await gridRows.nth(1).boundingBox();
  expect(firstRow).not.toBeNull();
  expect(secondRow).not.toBeNull();
  expect(secondRow?.y ?? 0).toBeGreaterThanOrEqual((firstRow?.y ?? 0) + (firstRow?.height ?? 0));
  await expect(page).toHaveScreenshot("family-grid-960x700.png", { fullPage: true });

  await gridRows.nth(1).evaluate((row) => {
    const scrollContainer = row.closest<HTMLElement>(".family-catalog-browser");
    if (!scrollContainer) return;
    const rowBounds = row.getBoundingClientRect();
    const containerBounds = scrollContainer.getBoundingClientRect();
    scrollContainer.scrollTop = Math.round(
      scrollContainer.scrollTop + rowBounds.top - containerBounds.top - 190,
    );
    scrollContainer.dispatchEvent(new Event("scroll"));
  });
  await page.evaluate(async () => {
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
    );
  });
  await expect(gridRows.nth(1)).toBeVisible();
  const controlsRemainTopmost = await page
    .locator(".family-catalog-controls")
    .evaluate((element) => {
      const bounds = element.getBoundingClientRect();
      const hit = document.elementFromPoint(
        bounds.left + bounds.width / 2,
        bounds.top + bounds.height / 2,
      );
      return hit === element || element.contains(hit);
    });
  expect(controlsRemainTopmost).toBe(true);
  await expect(page).toHaveScreenshot("family-grid-scrolled-960x700.png", { fullPage: true });

  await page.getByRole("radio", { name: "Vista familiar ultracompacta" }).click();
  await expect(page.locator(".family-catalog-browser--compact")).toBeVisible();
  await expect(page.locator(".family-game-row")).toHaveCount(12);
  await expect(page.getByRole("button", { name: "Abrir ficha de Aurora Assembly" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Abrir ficha de Bastion of Moss" })).toHaveCount(0);
  await app.expectNoHorizontalOverflow();

  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
    .analyze();
  const blocking = results.violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(blocking).toEqual([]);
  await expect(page).toHaveScreenshot("family-ultracompact-960x700.png", { fullPage: true });
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
