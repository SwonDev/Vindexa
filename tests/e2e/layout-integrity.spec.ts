import { expect, test } from "./support/fixtures";

/**
 * Guardas de maquetación que ningún test de unidad puede dar: comprueban que lo
 * que la persona necesita ver está **dentro de la ventana**, no simplemente
 * presente en el árbol. Un contenedor mal repartido deja el listado montado y
 * accesible por el DOM mientras la pantalla está en blanco.
 */
test.describe("integridad de la maquetación", () => {
  test("el listado empieza justo debajo de la barra y cabe en la ventana", async ({
    app,
    page,
  }) => {
    await app.goto();
    await app.waitForShell();
    await expect(app.gameButton("Nebula Frontier")).toBeVisible();

    const viewport = page.viewportSize();
    if (!viewport) throw new Error("La ventana de pruebas no declara tamaño.");

    const toolbar = page.locator(".library-toolbar");
    const surface = page.locator('[data-library-surface="true"]').first();
    const toolbarBox = await toolbar.boundingBox();
    const surfaceBox = await surface.boundingBox();
    expect(toolbarBox, "la barra de herramientas debe estar visible").not.toBeNull();
    expect(surfaceBox, "la superficie del listado debe estar visible").not.toBeNull();
    if (!toolbarBox || !surfaceBox) return;

    const toolbarBottom = toolbarBox.y + toolbarBox.height;
    // Entre la barra y el listado sólo cabe la fila de vistas guardadas.
    expect(surfaceBox.y - toolbarBottom).toBeLessThan(120);
    // Y el listado debe ocupar el resto de la ventana, no una franja al fondo.
    expect(surfaceBox.height).toBeGreaterThan(viewport.height * 0.5);

    const first = await app.gameButton("Nebula Frontier").boundingBox();
    expect(first, "la primera carátula debe estar visible").not.toBeNull();
    if (!first) return;
    expect(first.y).toBeLessThan(viewport.height * 0.6);
    expect(first.y + first.height).toBeLessThanOrEqual(viewport.height);
  });

  test("la biblioteca se desplaza en vertical", async ({ app, page }) => {
    await app.goto();
    await app.waitForShell();
    await expect(app.gameButton("Nebula Frontier")).toBeVisible();

    const surface = page.locator('[data-library-surface="true"]').first();
    const metrics = await surface.evaluate((node) => ({
      scrollHeight: node.scrollHeight,
      clientHeight: node.clientHeight,
      overflowY: getComputedStyle(node).overflowY,
    }));
    // El contenedor debe declarar el desplazamiento aunque el fixture sea corto:
    // sin esto, una biblioteca real queda sin forma de bajar.
    expect(["auto", "scroll", "overlay"]).toContain(metrics.overflowY);
    expect(metrics.clientHeight).toBeGreaterThan(200);
  });
});

/**
 * Reproduce el arranque de una biblioteca real: la sesión anterior dejó guardada
 * una posición de desplazamiento y hay muchos más juegos que los de la primera
 * página. Es el caso en el que la pantalla se quedaba en blanco con una fila
 * cortada bajo la barra.
 */
test.describe("restauración del desplazamiento guardado", () => {
  test.use({ scenario: "scale" });

  test("vuelve a la posición guardada con contenido a la vista", async ({ app, page }) => {
    await page.addInitScript(() => {
      window.localStorage.setItem(
        "vindexa:library-session:v1",
        JSON.stringify({
          state: {
            scope: { kind: "all", label: "Todos los juegos" },
            query: "",
            sort: "manual",
            randomSeed: 0,
            view: "grid",
            grouping: "none",
            familyAvailability: "all",
            familySort: "availability",
            filters: {},
          },
          scroll: { "all:all": 4200 },
          expanded: {},
        }),
      );
    });
    await app.goto();
    await app.waitForShell();

    const surface = page.locator('[data-library-surface="true"]').first();
    await expect(surface).toBeVisible();
    // Da tiempo a que el virtualizador aplique la posición restaurada.
    await page.waitForTimeout(400);

    const estado = await surface.evaluate((node) => {
      const filas = [...node.querySelectorAll(".virtual-grid-row")];
      const alto = node.clientHeight;
      const visibles = filas.filter((fila) => {
        const caja = fila.getBoundingClientRect();
        const contenedor = node.getBoundingClientRect();
        return caja.bottom > contenedor.top + 8 && caja.top < contenedor.bottom - 8;
      });
      return { scrollTop: node.scrollTop, filas: filas.length, visibles: visibles.length, alto };
    });

    // Si el contenedor se desplaza pero el virtualizador se queda en el origen,
    // sobreviven una o ninguna fila a la vista: eso es la pantalla en blanco.
    expect(estado.filas).toBeGreaterThan(1);
    expect(estado.visibles, "el listado no puede quedarse en blanco").toBeGreaterThanOrEqual(2);
  });
});

test.describe("separación de la rejilla", () => {
  test.use({ scenario: "scale" });

  test("ninguna carátula invade la fila de abajo", async ({ app, page }) => {
    await app.goto();
    await app.waitForShell();
    const surface = page.locator('[data-library-surface="true"]').first();
    await expect(surface).toBeVisible();
    await page.waitForTimeout(300);

    const medidas = await surface.evaluate((node) => {
      const filas = [...node.querySelectorAll(".virtual-grid-row")]
        .map((fila) => {
          const tarjeta = fila.querySelector(".game-card");
          const cajaFila = fila.getBoundingClientRect();
          const cajaTarjeta = tarjeta?.getBoundingClientRect();
          return {
            filaTop: cajaFila.top,
            tarjetaBottom: cajaTarjeta ? cajaTarjeta.bottom : cajaFila.bottom,
            altoTarjeta: cajaTarjeta ? cajaTarjeta.height : 0,
          };
        })
        .sort((izquierda, derecha) => izquierda.filaTop - derecha.filaTop);
      const huecos: number[] = [];
      for (let i = 0; i < filas.length - 1; i += 1) {
        const actual = filas[i];
        const siguiente = filas[i + 1];
        if (actual && siguiente) huecos.push(siguiente.filaTop - actual.tarjetaBottom);
      }
      return { huecos, altoTarjeta: filas[0]?.altoTarjeta ?? 0, filas: filas.length };
    });

    expect(medidas.filas).toBeGreaterThan(1);
    // El anillo de selección se dibuja hacia dentro, pero la fila de abajo aún
    // necesita aire suficiente para que las carátulas no se lean pegadas.
    for (const hueco of medidas.huecos) {
      expect(
        Math.round(hueco),
        `hueco real entre filas: ${medidas.huecos.join(", ")}`,
      ).toBeGreaterThanOrEqual(12);
    }
  });
});

/**
 * Un aviso que la persona no puede leer es peor que no darlo: la acción parece
 * no haber hecho nada. Este contrato recorre las pantallas y comprueba que
 * ningún elemento de estado queda tapado por el contenido que viene detrás.
 */
test.describe("los avisos de estado se leen", () => {
  const PANTALLAS = [
    "Biblioteca",
    "Planificador",
    "Colecciones",
    "Seguimiento",
    "Deseados",
  ] as const;

  test("ninguna pantalla esconde su propio aviso bajo el contenido", async ({ app, page }) => {
    await app.goto();
    await app.waitForShell();

    for (const pantalla of PANTALLAS) {
      await app.navigate(pantalla);
      await page.waitForTimeout(150);
      const tapados = await page.evaluate(() => {
        const avisos = [...document.querySelectorAll('[role="status"], [role="alert"]')];
        return avisos
          .filter((aviso) => {
            const caja = aviso.getBoundingClientRect();
            if (caja.width === 0 || caja.height === 0) return false;
            // Un elemento visualmente oculto para lectores de pantalla no cuenta.
            const estilo = getComputedStyle(aviso);
            if (estilo.position === "absolute" && caja.width <= 1) return false;
            // Quien esté en el centro del aviso tiene que ser el aviso o hijo suyo.
            const encima = document.elementFromPoint(
              caja.x + caja.width / 2,
              caja.y + caja.height / 2,
            );
            return Boolean(encima) && !aviso.contains(encima);
          })
          .map((aviso) => aviso.className || aviso.tagName);
      });
      expect(tapados, `avisos tapados en ${pantalla}`).toEqual([]);
    }
  });
});
