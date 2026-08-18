import React from "react";
import ReactDOM from "react-dom/client";
import { installWebviewChromeGuards } from "@/lib/webview-chrome";
import App from "./App";
import "./index.css";

// Suprime el cromo del webview —menú nativo, arrastre de imágenes, zoom con
// Ctrl/Cmd, selección accidental y «volver» por gesto— antes de montar la
// interfaz. Fuera de `useEffect` a propósito: en `StrictMode` un efecto se
// monta, desmonta y remonta, y en ese hueco reaparecería el menú del sistema.
installWebviewChromeGuards();

// Endurecimiento contra contaminación de prototipos, aquí y no en
// `app.security.freezePrototype`: ese ajuste de Tauri inyecta
// `Object.freeze(Object.prototype)` en *todos* los webviews, incluido el
// navegador de tiendas, y allí rompe el paquete de Steam —que sí escribe en
// `Object.prototype`— sin proteger nada nuestro, porque esa ventana no tiene
// IPC. Congelarlo aquí deja la garantía donde importa: la ventana propia.
Object.freeze(Object.prototype);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
