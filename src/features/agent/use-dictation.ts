/**
 * Dictar al agente en vez de escribirle.
 *
 * # Por qué existe
 *
 * Contar lo que has jugado es de las cosas que se dicen mejor hablando que
 * tecleando: «he estado dos horas con esto y voy por el cuarenta por ciento».
 * Escribirlo cuesta más que decirlo.
 *
 * # Qué hace y qué no
 *
 * Graba con el micrófono del sistema y manda el audio a un transcriptor que ya
 * esté corriendo en el ordenador. **El audio no se guarda en ninguna parte** y
 * no sale de la máquina: se graba, se transcribe y se olvida.
 *
 * Si no hay transcriptor, el botón ni siquiera aparece. Ofrecer algo que va a
 * fallar es peor que no ofrecerlo.
 *
 * # Por qué no se pide el micrófono al abrir
 *
 * El permiso se pide la primera vez que alguien pulsa dictar, no al montar el
 * panel: abrir una ventana no es motivo para que el sistema pregunte por el
 * micrófono.
 */

import { useCallback, useRef, useState } from "react";
import { api, getErrorMessage } from "@/lib/tauri";

export type DictationState = "idle" | "recording" | "transcribing";

export interface Dictation {
  state: DictationState;
  /** `undefined` mientras no se sepa; `null` cuando no hay transcriptor. */
  available: boolean;
  error?: string | undefined;
  start: () => Promise<void>;
  stop: () => void;
}

/** Formato que el navegador acepte grabar, de mejor a peor. */
function pickMimeType(): string {
  const candidates = ["audio/webm;codecs=opus", "audio/webm", "audio/mp4", "audio/ogg"];
  for (const candidate of candidates) {
    if (typeof MediaRecorder !== "undefined" && MediaRecorder.isTypeSupported(candidate)) {
      return candidate;
    }
  }
  return "audio/webm";
}

export function useDictation(
  baseUrl: string | undefined,
  onText: (text: string) => void,
): Dictation {
  const [state, setState] = useState<DictationState>("idle");
  const [error, setError] = useState<string>();
  const recorder = useRef<MediaRecorder>(null);
  const chunks = useRef<Blob[]>([]);

  const start = useCallback(async () => {
    if (!baseUrl || state !== "idle") return;
    setError(undefined);
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const mimeType = pickMimeType();
      const media = new MediaRecorder(stream, { mimeType });
      chunks.current = [];
      media.ondataavailable = (event) => {
        if (event.data.size > 0) chunks.current.push(event.data);
      };
      media.onstop = () => {
        // El micrófono se suelta en cuanto se deja de grabar: dejarlo abierto
        // enciende el indicador del sistema sin motivo.
        for (const track of stream.getTracks()) track.stop();
        const blob = new Blob(chunks.current, { type: mimeType });
        chunks.current = [];
        if (blob.size === 0) {
          setState("idle");
          return;
        }
        setState("transcribing");
        void blob
          .arrayBuffer()
          .then((buffer) => api.transcribeDictation(baseUrl, [...new Uint8Array(buffer)], mimeType))
          .then((text) => {
            if (text.trim()) onText(text.trim());
            else setError("No se entendió nada. Prueba a hablar más cerca del micrófono.");
          })
          .catch((cause) => setError(getErrorMessage(cause)))
          .finally(() => setState("idle"));
      };
      recorder.current = media;
      media.start();
      setState("recording");
    } catch (cause) {
      // Sin permiso de micrófono no hay dictado, y decirlo es más útil que
      // dejar el botón parpadeando.
      setError(
        cause instanceof DOMException && cause.name === "NotAllowedError"
          ? "Vindexa no tiene permiso para usar el micrófono."
          : getErrorMessage(cause),
      );
      setState("idle");
    }
  }, [baseUrl, onText, state]);

  const stop = useCallback(() => {
    recorder.current?.stop();
    recorder.current = null;
  }, []);

  return {
    state,
    available: Boolean(baseUrl),
    ...(error === undefined ? {} : { error }),
    start,
    stop,
  };
}
