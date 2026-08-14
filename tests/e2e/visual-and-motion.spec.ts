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
  const compactView = page.getByRole("button", { name: "Vista ultracompacta" });
  await compactView.click();
  await expect(compactView).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator(".game-browser--compact")).toBeVisible();
  await expect(page.locator(".game-row")).toHaveCount(3);
  await app.expectNoHorizontalOverflow();
  await expect(page).toHaveScreenshot("ultracompact-960x700.png", { fullPage: true });
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
