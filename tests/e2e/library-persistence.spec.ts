import { expect, test } from "./support/fixtures";

test("guarda progreso y checkpoint y los conserva tras reiniciar la interfaz", async ({
  app,
  page,
}) => {
  await app.goto();
  await app.openGame("Nebula Frontier");

  const dialog = page.getByRole("dialog");
  await dialog.getByRole("combobox", { name: "Estado personal" }).click();
  await page.getByRole("option", { name: "Jugando" }).click();
  await dialog.getByRole("combobox", { name: "Valoración personal" }).click();
  await page.getByRole("option", { name: "9/10" }).click();
  const checkpoint = dialog.getByLabel("¿Por dónde lo dejaste?");
  await checkpoint.fill("Entrada del observatorio, palanca izquierda activada.");
  const progress = dialog
    .locator(".slider-field")
    .filter({ hasText: "Progreso" })
    .getByRole("slider");
  await progress.focus();
  await progress.press("End");
  const save = dialog.getByRole("button", { name: "Guardar ahora" });
  await expect(save).toBeEnabled();
  await save.click();
  await page.waitForFunction(() => {
    const harness = (
      window as typeof window & {
        __VINDEXA_E2E__?: { snapshot: () => { commandLog: string[] } };
      }
    ).__VINDEXA_E2E__;
    return harness?.snapshot().commandLog.includes("update_game");
  });
  await expect(dialog.getByText("Guardado", { exact: true })).toBeVisible();

  await page.reload();
  await app.openGame("Nebula Frontier");
  await expect(page.getByRole("dialog").getByLabel("¿Por dónde lo dejaste?")).toHaveValue(
    "Entrada del observatorio, palanca izquierda activada.",
  );
  await expect(page.getByRole("dialog").getByText("100%", { exact: true })).toBeVisible();
});

test("arrastra a un estado, permite deshacer y persiste el segundo movimiento tras reiniciar", async ({
  app,
  page,
}) => {
  await app.goto();
  await app.dragGameToStatus("Nebula Frontier", "Terminados");
  await expect(
    page.getByRole("status").filter({ hasText: "movido a estado Terminados" }),
  ).toBeVisible();

  await page.getByRole("button", { name: "Deshacer" }).click();
  await expect(page.getByRole("status").filter({ hasText: "organización anterior" })).toBeVisible();
  await expect(app.gameButton("Nebula Frontier")).toHaveAccessibleName(/Jugando/);

  await app.dragGameToStatus("Nebula Frontier", "Terminados");
  await expect(
    page.getByRole("status").filter({ hasText: "movido a estado Terminados" }),
  ).toBeVisible();
  await page.reload();
  await expect(app.gameButton("Nebula Frontier")).toHaveAccessibleName(/Terminados/);
});

test("la sección de colecciones muestra organización manual e inteligente y conserva su orden", async ({
  app,
  page,
}) => {
  await app.goto();
  await app.navigate("Colecciones");
  await expect(page.getByRole("heading", { name: "Colecciones" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Favoritos" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Sesiones cortas" })).toBeVisible();

  await page.getByRole("button", { name: "Bajar Favoritos" }).click();
  await expect(page.locator(".collections-message")).toContainText("Orden de colecciones guardado");
  await page.reload();
  await app.navigate("Colecciones");
  await expect(page.getByRole("heading", { name: "Sesiones cortas" })).toBeVisible();
  const headings = await page.locator(".collection-tile__name").allTextContents();
  expect(headings).toEqual(["Sesiones cortas", "Favoritos"]);
});
