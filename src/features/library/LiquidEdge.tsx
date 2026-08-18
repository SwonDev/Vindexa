import { useMemo } from "react";

/**
 * Lente del borde superior de la biblioteca.
 *
 * Al desplazar, las carátulas no se cortan contra la barra: entran bajo una
 * lámina de vidrio que las difumina de forma **progresiva** y las funde entre
 * sí. Una sola capa de desenfoque no da esa lectura —se ve como un cristal
 * esmerilado plano—; lo que la da es apilar varias capas con desenfoque
 * creciente, cada una recortada a su propia franja, de modo que la nitidez cae
 * de forma continua desde el contenido hasta el filo.
 *
 * La deformación por refracción no puede venir de un filtro SVG: WebKit —el
 * motor de la aplicación de escritorio— **ignora `url()` dentro de
 * `backdrop-filter`**, así que un mapa de desplazamiento no se aplicaría. Lo
 * que sí se puede es lo que hace un vidrio real en el filo: separar levemente
 * los colores. Esa aberración cromática es la capa que convierte un desenfoque
 * en algo que se lee como refracción.
 *
 * La intensidad la manda `--scroll-fade`, que escribe el manejador de
 * desplazamiento del listado (0 arriba del todo, 1 desplazado): el efecto no
 * cuesta un renderizado de React por fotograma.
 */

/** Número de capas. Menos de cuatro se nota escalonado; más de ocho no se ve. */
const CAPAS = 6;

/** Multiplica el desenfoque de todas las capas. */
const FUERZA = 2.4;

/**
 * Suavizado de la progresión. La curva en ese entra y sale despacio, que es lo
 * que evita que la última capa dé un salto contra el filo.
 */
function suavizar(progreso: number): number {
  return progreso * progreso * (3 - 2 * progreso);
}

interface CapaLente {
  desenfoque: number;
  mascara: string;
}

function construirCapas(): CapaLente[] {
  const paso = 100 / CAPAS;
  return Array.from({ length: CAPAS }, (_, indice) => {
    const numero = indice + 1;
    const progreso = suavizar(numero / CAPAS);
    // Progresión exponencial: el filo tiene que ir mucho más desenfocado que el
    // arranque, y una rampa lineal deja el borde demasiado legible.
    const desenfoque = 2 ** (progreso * 4) * 0.0625 * FUERZA;

    // Cada capa se recorta a su franja y se solapa con las vecinas, así que las
    // transiciones entre niveles de desenfoque no dejan costura.
    const inicio = Math.round((paso * numero - paso) * 10) / 10;
    const plenoDesde = Math.round(paso * numero * 10) / 10;
    const plenoHasta = Math.round((paso * numero + paso) * 10) / 10;
    const fin = Math.round((paso * numero + paso * 2) * 10) / 10;

    let paradas = `transparent ${inicio}%, #000 ${plenoDesde}%`;
    if (plenoHasta <= 100) paradas += `, #000 ${plenoHasta}%`;
    if (fin <= 100) paradas += `, transparent ${fin}%`;

    return { desenfoque, mascara: `linear-gradient(to top, ${paradas})` };
  });
}

export function LiquidEdge() {
  const capas = useMemo(construirCapas, []);
  return (
    <div className="liquid-edge" aria-hidden="true">
      {capas.map((capa) => (
        <span
          key={capa.desenfoque}
          className="liquid-edge__capa"
          style={
            {
              "--capa-desenfoque": `${capa.desenfoque.toFixed(3)}rem`,
              maskImage: capa.mascara,
              WebkitMaskImage: capa.mascara,
            } as React.CSSProperties
          }
        />
      ))}
      <span className="liquid-edge__cromatica" />
      <span className="liquid-edge__filo" />
    </div>
  );
}
