export type ChatMessage = {
  role: "user" | "assistant";
  content: string;
};

type ChatResponse = { content: string };
type EmbeddingResponse = { embedding: number[]; dimensions: number };
export type KnowledgeExtraction = { should_store: boolean; content: string | null };
export type DocumentChunk = { content: string; chunk_index: number; token_count: number };

const BACKEND_URL = "http://127.0.0.1:8000";
const pendingRequests = new Set<AbortController>();

export function cancelPendingRequests(): void {
  for (const controller of pendingRequests) controller.abort();
  pendingRequests.clear();
}

const wait = (milliseconds: number) => new Promise<void>((resolve) => {
  window.setTimeout(resolve, milliseconds);
});

export async function waitForBackendReady(timeoutMs = 180_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastDetail = "FastAPI todavía no está disponible.";
  while (Date.now() < deadline) {
    const controller = new AbortController();
    pendingRequests.add(controller);
    try {
      const response = await fetch(`${BACKEND_URL}/health`, { signal: controller.signal });
      if (response.ok) {
        const payload = (await response.json()) as { status?: string };
        if (payload.status === "ready") return;
        if (payload.status === "failed") {
          throw new Error("FastAPI no pudo preparar los modelos.");
        }
        lastDetail = "Los modelos todavía se están preparando.";
      }
    } catch (error) {
      if (error instanceof DOMException && error.name === "AbortError") {
        throw new Error("La operación fue cancelada.");
      }
      if (error instanceof Error && error.message.includes("no pudo preparar")) throw error;
      lastDetail = "FastAPI todavía no está disponible.";
    } finally {
      pendingRequests.delete(controller);
    }
    await wait(1_000);
  }
  throw new Error(`${lastDetail} Se agotó el tiempo de espera.`);
}

async function postJson<T>(path: string, body: object): Promise<T> {
  let response: Response;
  const controller = new AbortController();
  pendingRequests.add(controller);
  try {
    response = await fetch(`${BACKEND_URL}${path}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal: controller.signal,
    });
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      throw new Error("La operación fue cancelada.");
    }
    throw new Error(
      "No fue posible conectar con FastAPI. Verifica que esté ejecutándose en 127.0.0.1:8000.",
    );
  } finally {
    pendingRequests.delete(controller);
  }
  if (!response.ok) {
    let detail = `El backend respondió con el estado ${response.status}.`;
    try {
      const payload = (await response.json()) as { detail?: string };
      if (payload.detail) detail = payload.detail;
    } catch {
      // Preserve the HTTP fallback.
    }
    if (response.status === 404) {
      detail = `El FastAPI activo no contiene ${path}. Reinícialo.`;
    }
    throw new Error(detail);
  }
  return (await response.json()) as T;
}

export async function completeChat(
  messages: ChatMessage[],
  memories: string[] = [],
): Promise<string> {
  const response = await postJson<ChatResponse>("/api/chat", { messages, memories });
  if (!response.content?.trim()) {
    throw new Error("El modelo devolvió una respuesta vacía.");
  }
  return response.content.trim();
}

export async function createEmbedding(text: string): Promise<number[]> {
  const response = await postJson<EmbeddingResponse>("/api/embeddings", { text });
  if (response.dimensions !== response.embedding.length) {
    throw new Error("El backend devolvió un embedding inconsistente.");
  }
  return response.embedding;
}

export function extractKnowledge(messages: ChatMessage[]): Promise<KnowledgeExtraction> {
  return postJson<KnowledgeExtraction>("/api/knowledge/extract", { messages });
}

export async function extractDocumentChunks(
  path: string,
  extension: string,
): Promise<DocumentChunk[]> {
  const response = await postJson<{ chunks: DocumentChunk[] }>("/api/documents/chunks", {
    path,
    extension,
  });
  return response.chunks;
}
