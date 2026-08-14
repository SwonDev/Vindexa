import { expect, test } from "./support/fixtures";

test.describe("arranque aislado y estados de biblioteca", () => {
  test.use({ scenario: "empty" });

  test("la primera ejecución explica cómo construir la biblioteca sin tocar datos reales", async ({
    app,
    page,
  }) => {
    await app.goto();
    await app.waitForShell();

    await expect(page.getByRole("heading", { name: "Construye tu biblioteca real" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Importar Steam local" })).toBeVisible();
    await expect(page.getByText("0 juegos · 0 instalados")).toBeVisible();
    await app.expectNoHorizontalOverflow();
  });
});

test.describe("biblioteca poblada", () => {
  test("carga juegos reales del fixture mediante el contrato Tauri", async ({ app, page }) => {
    await app.goto();
    await app.waitForShell();

    await expect(app.gameButton("Nebula Frontier")).toBeVisible();
    await expect(app.gameButton("Clockwork Harbor")).toBeVisible();
    await expect(page.getByText("3 juegos · 2 instalados")).toBeVisible();
    await app.expectNoHorizontalOverflow();
  });
});

test.describe("error y recuperación de arranque", () => {
  test.use({ scenario: "startup-recovery" });

  test("permite reintentar tras un fallo transitorio y recupera la biblioteca", async ({
    app,
    page,
  }) => {
    await app.goto();
    await expect(
      page.getByRole("heading", { name: "No se pudo abrir la biblioteca" }),
    ).toBeVisible();
    await expect(page.getByText("El arranque aislado falló una vez.")).toBeVisible();

    await page.getByRole("button", { name: "Reintentar" }).click();
    await expect(app.gameButton("Nebula Frontier")).toBeVisible();
  });
});

test.describe("error de consulta", () => {
  test.use({ scenario: "library-error" });

  test("presenta un error accionable sin mezclarlo con un estado vacío", async ({ app, page }) => {
    await app.goto();
    await expect(
      page.getByRole("heading", { name: "No se pudieron cargar los juegos" }),
    ).toBeVisible();
    await expect(page.getByText("La consulta aislada de biblioteca falló.")).toBeVisible();
    await expect(page.getByRole("heading", { name: "Construye tu biblioteca real" })).toHaveCount(
      0,
    );
  });
});

test.describe("recuperación protectora de la base", () => {
  test.use({ scenario: "database-recovery" });

  test("bloquea la aplicación y solo restaura tras la confirmación explícita", async ({
    app,
    page,
  }) => {
    await app.goto();
    await expect(
      page.getByRole("heading", { name: "Recuperación de datos necesaria" }),
    ).toBeVisible();
    await expect(page.getByText("SQLite detectó daños en la copia aislada.")).toBeVisible();
    await expect(page.getByText("Copia verificada", { exact: true })).toBeVisible();
    await expect(app.shell).toHaveCount(0);

    await page.getByRole("button", { name: "Restaurar esta copia" }).click();
    await page.getByLabel("Confirmación de restauración").fill("RESTAURAR");
    await page.getByRole("button", { name: "Confirmar restauración" }).click();
    await app.waitForShell();
    await expect(app.gameButton("Nebula Frontier")).toBeVisible();
  });
});
