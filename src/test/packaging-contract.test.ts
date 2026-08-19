import { readFileSync, statSync } from "node:fs";
import { describe, expect, it } from "vitest";

/**
 * Lo que hace que un instalador se pueda instalar.
 *
 * # Los fallos que la motivaron
 *
 * En Fedora, la versión publicada daba error al instalar y el icono de la
 * aplicación salía roto y antiguo. Dos causas distintas, las dos en este
 * archivo de configuración y ninguna visible desde macOS:
 *
 * 1. La lista de iconos incluía `128x128@2x.png`. El empaquetador de Linux
 *    traduce cada nombre a un directorio de `hicolor`, y de ahí salía
 *    `hicolor/256x256@2/apps/`, que no es un tamaño válido del estándar
 *    freedesktop: el escritorio no encuentra el icono y cae en uno genérico.
 * 2. El RPM declaraba dos bibliotecas cuando el binario necesita ocho. Al
 *    instalar, `dnf` resuelve lo declarado, no lo real.
 *
 * # Por qué se comprueba aquí
 *
 * Porque desde macOS no se puede instalar un RPM, y esperar a que alguien lo
 * instale para descubrir que está roto ya pasó una vez. Esto no reemplaza a
 * compilar en las tres plataformas —eso lo hace la integración continua—, pero
 * sí impide volver a publicar la configuración que rompió Fedora.
 */

const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")) as {
  bundle: {
    icon: string[];
    category?: string;
    linux?: { deb?: { depends?: string[] }; rpm?: { depends?: string[] } };
  };
};

/** Ancho y alto reales de un PNG, leídos de su cabecera IHDR. */
function tamañoPng(ruta: string): { ancho: number; alto: number } {
  const bytes = readFileSync(ruta);
  // 8 bytes de firma + 4 de longitud + 4 de tipo ("IHDR") → ancho y alto.
  return { ancho: bytes.readUInt32BE(16), alto: bytes.readUInt32BE(20) };
}

describe("iconos del instalador", () => {
  const pngs = config.bundle.icon.filter((ruta) => ruta.endsWith(".png"));

  it("ningún nombre produce un directorio de hicolor inválido", () => {
    // `WxH` y nada más: cualquier sufijo acaba en un directorio que el
    // escritorio no reconoce, y el icono se pierde sin error.
    const inválidos = pngs.filter(
      (ruta) => !/^icons\/\d+x\d+\.png$/.test(ruta) && ruta !== "icons/icon.png",
    );
    expect(inválidos).toEqual([]);
  });

  it("cada icono existe y mide lo que su nombre promete", () => {
    // Un 512 que en realidad mide 256 se escala y se ve sucio justo donde más
    // se mira: el lanzador y la tienda de aplicaciones.
    for (const ruta of pngs) {
      const nombre = ruta.replace("icons/", "").replace(".png", "");
      const [ancho, alto] = nombre.split("x").map(Number);
      const real = tamañoPng(`src-tauri/${ruta}`);
      expect({ ruta, ...real }).toEqual({ ruta, ancho, alto });
    }
  });

  it("están los tamaños que piden los tres escritorios", () => {
    for (const tamaño of [16, 32, 48, 128, 256, 512]) {
      expect(pngs).toContain(`icons/${tamaño}x${tamaño}.png`);
    }
  });

  it("hay icono nativo de macOS y de Windows", () => {
    expect(config.bundle.icon).toContain("icons/icon.icns");
    expect(config.bundle.icon).toContain("icons/icon.ico");
    // Un `.icns` de decenas de kilobytes es una máscara que falló: el archivo
    // bueno pesa megabytes porque lleva todas las resoluciones dentro.
    expect(statSync("src-tauri/icons/icon.icns").size).toBeGreaterThan(500_000);
  });
});

describe("dependencias declaradas en Linux", () => {
  // El binario enlaza contra estas familias. Si alguna falta en la lista, el
  // paquete instala y la aplicación no arranca —o ni instala—.
  const FAMILIAS = [
    "webkit",
    "javascriptcore",
    "soup",
    "gtk",
    "gdk-pixbuf",
    "cairo",
    "dbus",
    "rsvg",
  ];

  it.each([
    ["deb", config.bundle.linux?.deb?.depends ?? []],
    ["rpm", config.bundle.linux?.rpm?.depends ?? []],
  ])("%s las declara todas", (_formato, depends) => {
    const faltan = FAMILIAS.filter(
      (familia) => !depends.some((paquete) => paquete.includes(familia)),
    );
    expect(faltan).toEqual([]);
  });
});

describe("metadatos del escritorio", () => {
  it("declara categoría, o queda sin sitio en el menú de aplicaciones", () => {
    expect(config.bundle.category).toBe("Game");
  });
});
