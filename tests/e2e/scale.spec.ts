import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "./support/fixtures";

/**
 * Prueba de escala: mil quinientos juegos.
 *
 * Una biblioteca de cuarenta y ocho títulos no demuestra nada. Esto mide lo que
 * de verdad importa cuando la biblioteca es grande: cuánto tarda en aparecer la
 * primera pantalla, cuántas filas monta el virtualizador y si desplazarse hasta
 * el fondo sigue siendo instantáneo.
 */
const OUT = join(process.cwd(), "artifacts", "showcase");
mkdirSync(OUT, { recursive: true });

test.use({ scenario: "scale" });

test("la biblioteca sigue siendo utilizable con 1.500 juegos", async ({ app, page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });

  const started = Date.now();
  await app.goto();
  await app.waitForShell();
  await expect(page.locator(".game-card").first()).toBeVisible();
  const firstPaint = Date.now() - started;

  // El virtualizador solo debe montar lo visible, no las mil quinientas fichas.
  const mounted = await page.locator(".game-card").count();
  expect(mounted, "el virtualizador monta solo lo visible").toBeLessThan(60);

  // El recuento refleja el tamaño real de la biblioteca, no la página cargada.
  // Vive en la barra lateral desde que la de estado dejó de repetir lo que ya
  // se ve en otro sitio. Sin separador de millares: en español, cuatro cifras
  // no lo llevan, y `toLocaleString("es-ES")` lo respeta.
  await expect(page.getByRole("button", { name: /^Todos los juegos/ })).toContainText("1500");

  // Desplazarse muy lejos no puede degradar: se mide el salto al fondo.
  const scrollStarted = Date.now();
  await page.locator(".game-browser").evaluate((node) => {
    node.scrollTop = node.scrollHeight;
    node.dispatchEvent(new Event("scroll"));
  });
  await page.waitForTimeout(220);
  const scrolled = Date.now() - scrollStarted;
  const stillMounted = await page.locator(".game-card").count();

  console.log(
    `escala · primera pantalla ${firstPaint} ms · fichas montadas ${mounted} · ` +
      `salto al fondo ${scrolled} ms · fichas tras el salto ${stillMounted}`,
  );

  expect(stillMounted, "el desplazamiento no acumula nodos").toBeLessThan(60);
  // Presupuestos generosos, pensados para detectar una regresión de orden de
  // magnitud, no para medir la máquina: hoy son ~575 ms y ~336 ms.
  expect(firstPaint, "la primera pantalla no puede tardar segundos").toBeLessThan(4_000);
  expect(scrolled, "saltar al fondo debe ser inmediato").toBeLessThan(2_000);
  await app.expectNoHorizontalOverflow();
  await page.screenshot({ path: join(OUT, "escala-1500-1440x900.png") });
});

test("agrupar 1.500 juegos por inicial sigue respondiendo", async ({ app, page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await app.goto();
  await app.waitForShell();

  // El conmutador de vista es un grupo de radios nativo, no tres botones: así
  // el lector de pantalla dice «1 de 3» y las flechas funcionan solas.
  await page.getByRole("radio", { name: "Vista de lista" }).click();
  const started = Date.now();
  await page.getByRole("combobox", { name: "Agrupar biblioteca" }).click();
  await page.getByRole("option", { name: "Inicial" }).click();
  await expect(page.locator(".library-group-header").first()).toBeVisible();
  console.log(`escala · agrupación aplicada en ${Date.now() - started} ms`);

  await page.screenshot({ path: join(OUT, "escala-agrupada-1440x900.png") });
});
