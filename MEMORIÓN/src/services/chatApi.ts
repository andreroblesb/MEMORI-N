export type ChatMessage = {
  role: "user" | "assistant";
  content: string;
};

type ChatResponse = {
  content: string;
};

const BACKEND_URL = "http://127.0.0.1:8000";

export async function completeChat(messages: ChatMessage[]): Promise<string> {
  let response: Response;
  try {
    response = await fetch(`${BACKEND_URL}/api/chat`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ messages }),
    });
  } catch {
    throw new Error(
      "No fue posible conectar con FastAPI. Verifica que esté ejecutándose en 127.0.0.1:8000.",
    );
  }

  if (!response.ok) {
    let detail = `El backend respondió con el estado ${response.status}.`;
    try {
      const body = (await response.json()) as { detail?: string };
      if (body.detail) detail = body.detail;
    } catch {
      // Preserve the HTTP status fallback when the body is not JSON.
    }
    if (response.status === 404) {
      detail =
        "El FastAPI en el puerto 8000 es una versión anterior y no contiene /api/chat. Reinícialo.";
    }
    throw new Error(detail);
  }

  const body = (await response.json()) as ChatResponse;
  if (!body.content?.trim()) {
    throw new Error("El modelo devolvió una respuesta vacía.");
  }
  return body.content.trim();
}
