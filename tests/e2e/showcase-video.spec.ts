import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "./support/fixtures";

/**
 * Vídeos de vitrina para la portada del repositorio.
 *
 * Vive aparte de `showcase.spec.ts` porque graba vídeo, y grabar todos los
 * recorridos de capturas sería mucho peso por nada. Igual que la vitrina de
 * imágenes, no comprueba nada: enseña la aplicación real moviéndose, con el
 * catálogo sembrado y arte oficial.
 *
 * El vídeo lo escribe Playwright en su `outputDir`; `scripts/vitrina.sh` lo
 * recoge de ahí, lo enmarca y lo convierte.
 */
const OUT = join(process.cwd(), "artifacts", "showcase-video");
mkdirSync(OUT, { recursive: true });

test.use({
  scenario: "showcase",
  video: { mode: "on", size: { width: 1440, height: 900 } },
});
test.describe.configure({ mode: "serial" });

async function esperarArte(page: import("@playwright/test").Page) {
  await page.evaluate(() => document.fonts.ready);
  // Basta con que la mayoría del arte esté resuelto: alguna portada puede no
  // existir en la CDN y bloquear la grabación indefinidamente.
  await page
    .waitForFunction(
      () => {
        const imagenes = Array.from(document.querySelectorAll("img"));
        const visibles = imagenes.filter((imagen) => imagen.getBoundingClientRect().width > 0);
        if (!visibles.length) return false;
        return visibles.filter((imagen) => imagen.complete).length / visibles.length >= 0.85;
      },
      undefined,
      { timeout: 20_000 },
    )
    .catch(() => undefined);
  await page.waitForTimeout(500);
}

/** Desplaza el listado poco a poco, como lo haría una persona. */
async function desplazar(page: import("@playwright/test").Page, hasta: number, pasos: number) {
  for (let paso = 1; paso <= pasos; paso += 1) {
    await page.locator(".game-browser").evaluate(
      (nodo, top) => {
        nodo.scrollTop = top;
        nodo.dispatchEvent(new Event("scroll"));
      },
      Math.round((hasta * paso) / pasos),
    );
    await page.waitForTimeout(220);
  }
}

test("vídeo · recorrido por la biblioteca", async ({ app, page }) => {
  await app.goto();
  await app.waitForShell();
  await esperarArte(page);
  await page.waitForTimeout(900);

  await desplazar(page, 1600, 12);
  await page.waitForTimeout(400);
  await desplazar(page, 0, 6);
  await page.waitForTimeout(700);
});

test("vídeo · densidades y agrupación", async ({ app, page }) => {
  await app.goto();
  await app.waitForShell();
  await esperarArte(page);
  await page.waitForTimeout(700);

  for (const nombre of ["Lista", "Ultracompacta", "Cuadrícula"]) {
    const boton = page.getByRole("button", { name: new RegExp(nombre, "i") }).first();
    if (await boton.isVisible().catch(() => false)) {
      await boton.click();
      await page.waitForTimeout(1100);
    }
  }
  await page.waitForTimeout(600);
});

test("vídeo · ficha de un juego", async ({ app, page }) => {
  await app.goto();
  await app.waitForShell();
  await esperarArte(page);
  await page.waitForTimeout(700);

  await app.openGame("ELDEN RING");
  const ficha = page.getByRole("dialog");
  await expect(ficha).toBeVisible();
  await page.waitForTimeout(1400);

  // El contenedor desplazable de la ficha se busca por sus dimensiones, igual
  // que en la vitrina de imágenes: no tiene un selector estable.
  await ficha.evaluate((dialogo) => {
    const candidatos = [dialogo, ...dialogo.querySelectorAll<HTMLElement>("*")] as HTMLElement[];
    const desplazable = candidatos.find(
      (nodo) => nodo.scrollHeight > nodo.clientHeight + 40 && nodo.clientHeight > 160,
    );
    desplazable?.scrollTo({ top: 640, behavior: "smooth" });
  });
  await page.waitForTimeout(1800);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(800);
});

test("vídeo · paleta de comandos", async ({ app, page }) => {
  await app.goto();
  await app.waitForShell();
  await esperarArte(page);
  await page.waitForTimeout(600);

  await page.getByRole("button", { name: "Abrir la paleta de comandos" }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.waitForTimeout(700);
  const campo = page.getByRole("dialog").getByRole("combobox");
  for (const letra of "planificador") {
    await campo.press(letra);
    await page.waitForTimeout(80);
  }
  await page.waitForTimeout(1200);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(700);
});
