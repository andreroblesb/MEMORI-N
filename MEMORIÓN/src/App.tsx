import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ActionIcon,
  Avatar,
  Badge,
  Box,
  Button,
  Card,
  Center,
  Checkbox,
  Group,
  Loader,
  Modal,
  Paper,
  ScrollArea,
  SimpleGrid,
  Stack,
  Text,
  TextInput,
  Textarea,
  ThemeIcon,
  Title,
  Tooltip,
} from "@mantine/core";
import {
  IconActivity,
  IconChartBar,
  IconEdit,
  IconFlag,
  IconFileText,
  IconFolder,
  IconInfoCircle,
  IconMessage,
  IconPlus,
  IconRefresh,
  IconSearch,
  IconSend,
  IconSettings,
  IconTrash,
  IconUser,
} from "@tabler/icons-react";
import "./App.css";
import { MarkdownMessage } from "./components/MarkdownMessage";
import { cancelPendingRequests, completeChat, createEmbedding, extractDocumentChunks, extractKnowledge, waitForBackendReady } from "./services/chatApi";

type View = "chat" | "analytics";
type Folder = {
  id: number;
  name: string;
  canonicalPath: string;
  scanStatus: string;
  lastError: string | null;
  lastScannedAt: string | null;
  extensions: string[];
};
type Message = { role: "user" | "assistant"; content: string };
const INTRO_MESSAGE: Message = {
  role: "assistant",
  content: [
    "Soy **MEMORIÓN**, tu asesor personal de memoria.",
    "",
    "Puedo ayudarte a:",
    "",
    "- **Recordar información:** cuéntame por texto algo importante que quieras conservar o adjunta un documento.",
    "- **Recuperar información:** pregúntame por datos que me hayas pedido recordar anteriormente.",
    "",
    "¿Quieres contarme algo para recordarlo o buscar algo que ya me compartiste?",
  ].join("\n"),
};
type DocumentRecord = {
  id: number;
  scope: "general" | "folder";
  folderId: number | null;
  relativePath: string;
  canonicalPath: string;
  volumeId: string | null;
  fileId: string | null;
  managedCopy: boolean;
  indexingStatus: string;
};
type SystemMetrics = { cpuPercent: number; ramUsedBytes: number; ramTotalBytes: number };
type ActivityMetrics = {
  folderChatCount: number;
  sessionMessageCount: number;
  sessionTextBytes: number;
  mappedFileCount: number;
  mappedDirectoryCount: number;
  mappedFolderBytes: number;
  inaccessibleEntryCount: number;
};
type KnowledgeMatch = { knowledge: { content: string }; distance: number };
type ScanCandidate = {
  documentId: number;
  canonicalPath: string;
  relativePath: string;
  extension: string;
};

const SUPPORTED_DOCUMENT_FORMATS = [".pdf", ".docx", ".json", ".md", ".txt", ".pptx", ".rtf", ".xml"];
const ISSUES_URL = "https://github.com/andreroblesb/MEMORI-N/issues/new";
const scanLabel = (status: string) => ({
  pending: "Pendiente",
  scanning: "Indexando",
  completed: "Indexada",
  failed: "Con errores de indexación. Reporte el problema.",
}[status] ?? status);
const scanColor = (status: string) => ({
  pending: "yellow",
  scanning: "violet",
  completed: "teal",
  failed: "red",
}[status] ?? "gray");

const inTauri = () => "__TAURI_INTERNALS__" in window;

function recentKnowledgeContext(messages: Message[]): Message[] {
  const selected: Message[] = [];
  let users = 0;
  let assistants = 0;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message.role === "user" && users < 2) {
      selected.push(message);
      users += 1;
    } else if (message.role === "assistant" && assistants < 2) {
      selected.push(message);
      assistants += 1;
    }
    if (users === 2 && assistants === 2) break;
  }
  return selected.reverse();
}

function App() {
  const [view, setView] = useState<View>("chat");
  const [folders, setFolders] = useState<Folder[]>([]);
  const [selectedFolder, setSelectedFolder] = useState<number | null>(null);
  const [folderModal, setFolderModal] = useState(false);
  const [folderBusy, setFolderBusy] = useState(false);
  const [folderError, setFolderError] = useState("");
  const [editingFolders, setEditingFolders] = useState(false);
  const [settingsModal, setSettingsModal] = useState(false);
  const [folderDraft, setFolderDraft] = useState<{ id?: number; name: string; path: string } | null>(null);
  const [folderExtensions, setFolderExtensions] = useState<string[]>(SUPPORTED_DOCUMENT_FORMATS.map((format) => format.slice(1)));
  const [confirmFolderDelete, setConfirmFolderDelete] = useState(false);
  const [prompt, setPrompt] = useState("");
  const [messages, setMessages] = useState<Message[]>([INTRO_MESSAGE]);
  const [chatBusy, setChatBusy] = useState(false);
  const [busyChatFolderId, setBusyChatFolderId] = useState<number | null | undefined>(undefined);
  const [chatError, setChatError] = useState("");
  const [documents, setDocuments] = useState<DocumentRecord[]>([]);
  const [pendingAttachment, setPendingAttachment] = useState<string | null>(null);
  const [attachmentBusy, setAttachmentBusy] = useState(false);
  const [attachmentError, setAttachmentError] = useState("");
  const [search, setSearch] = useState("");
  const selectedFolderRef = useRef<number | null>(null);
  const startupScanStarted = useRef(false);

  const replaceFolder = (folder: Folder) => {
    setFolders((current) => [...current.filter((item) => item.id !== folder.id), folder]
      .sort((a, b) => a.name.localeCompare(b.name)));
  };

  const reportIssue = async () => {
    if (inTauri()) {
      await openUrl(ISSUES_URL);
    } else {
      window.open(ISSUES_URL, "_blank", "noopener,noreferrer");
    }
  };

  const activeFolder = useMemo(
    () => folders.find((folder) => folder.id === selectedFolder),
    [folders, selectedFolder],
  );

  useEffect(() => {
    if (!inTauri()) return;
    Promise.all([
      invoke<Folder[]>("list_folders"),
      invoke<Message[]>("list_session_messages", { folderId: null }),
    ]).then(([nextFolders, nextMessages]) => {
      if (nextFolders.length === 0) startupScanStarted.current = true;
      setFolders(nextFolders);
      setMessages(nextMessages.length > 0 ? nextMessages : [INTRO_MESSAGE]);
    }).catch((error) => setFolderError(String(error)));
  }, []);

  useEffect(() => {
    const cancel = () => cancelPendingRequests();
    window.addEventListener("beforeunload", cancel);
    const closeListener = inTauri()
      ? getCurrentWindow().onCloseRequested(cancel)
      : null;
    return () => {
      window.removeEventListener("beforeunload", cancel);
      cancel();
      void closeListener?.then((unlisten) => unlisten());
    };
  }, []);

  const openChat = async (folderId: number | null) => {
    selectedFolderRef.current = folderId;
    setSelectedFolder(folderId);
    setView("chat");
    setDocuments([]);
    setPendingAttachment(null);
    if (!inTauri()) {
      setMessages([INTRO_MESSAGE]);
      setDocuments([]);
      return;
    }
    try {
      const nextMessages = await invoke<Message[]>("list_session_messages", { folderId });
      setMessages(nextMessages.length > 0 ? nextMessages : [INTRO_MESSAGE]);
    } catch (error) {
      console.error(error);
      setMessages([INTRO_MESSAGE]);
      setDocuments([]);
    }
  };

  const submitPrompt = async (value = prompt) => {
    const clean = value.trim();
    if (!clean || chatBusy) return;
    const targetFolder = selectedFolder;
    const nextMessages: Message[] = [...messages, { role: "user", content: clean }];
    setPrompt("");
    setChatError("");
    setChatBusy(true);
    setBusyChatFolderId(targetFolder);
    setMessages(nextMessages);
    setDocuments([]);
    try {
      if (inTauri()) {
        await invoke("append_session_message", {
          folderId: targetFolder,
          role: "user",
          content: clean,
        });
      }
      let memories: string[] = [];
      if (inTauri()) {
        const queryEmbedding = await createEmbedding(clean);
        const matches = await invoke<KnowledgeMatch[]>("search_knowledge", {
          embedding: queryEmbedding,
          scope: targetFolder === null ? "general" : "folder",
          folderId: targetFolder,
          limit: 8,
        });
        memories = matches
          .filter((match) => match.distance <= 0.65)
          .map((match) => match.knowledge.content);
      }
      const content = await completeChat(nextMessages.slice(-20), memories);
      if (inTauri()) {
        await invoke("append_session_message", {
          folderId: targetFolder,
          role: "assistant",
          content,
        });
      }
      if (selectedFolderRef.current === targetFolder) {
        setMessages((current) => [...current, { role: "assistant", content }]);
      }
      if (inTauri()) {
        const contextMessages = recentKnowledgeContext(messages);
        void (async () => {
          const extraction = await extractKnowledge([
            ...contextMessages,
            { role: "user", content: clean },
          ]);
          if (!extraction.should_store || !extraction.content) return;
          const embedding = await createEmbedding(extraction.content);
          await invoke("store_general_chat_knowledge", {
            folderId: targetFolder,
            input: {
              userInput: clean,
              content: extraction.content,
              contextMessages,
              embedding,
            },
          });
        })().catch((error) => console.error("No se pudo crear knowledge item", error));
      }
    } catch (error) {
      setChatError(error instanceof Error ? error.message : String(error));
    } finally {
      setChatBusy(false);
      setBusyChatFolderId(undefined);
    }
  };

  const selectFolder = async () => {
    if (!inTauri()) {
      setFolderError("La selección de carpetas está disponible en la aplicación de escritorio.");
      return;
    }
    setFolderError("");
    try {
      const selected = await open({ directory: true, multiple: false, recursive: true, title: "Selecciona una carpeta para MEMORIÓN" });
      if (!selected) return;
      const normalized = selected.replace(/[\\/]+$/, "");
      const name = normalized.split(/[\\/]/).pop() || normalized;
      setFolderDraft({ name, path: normalized });
      setFolderExtensions(SUPPORTED_DOCUMENT_FORMATS.map((format) => format.slice(1)));
      setConfirmFolderDelete(false);
      setFolderModal(true);
    } catch (error) {
      setFolderError(String(error));
    }
  };

  const editFolderConfiguration = (folder: Folder) => {
    setFolderDraft({ id: folder.id, name: folder.name, path: folder.canonicalPath });
    setFolderExtensions(folder.extensions);
    setConfirmFolderDelete(false);
    setFolderError("");
    setFolderModal(true);
  };

  const scanFolder = async (folder: Folder) => {
    replaceFolder({ ...folder, scanStatus: "scanning", lastError: null });
    let scanError: string | null = null;
    try {
      const candidates = await invoke<ScanCandidate[]>("prepare_folder_scan", { folderId: folder.id });
      if (candidates.length > 0) await waitForBackendReady();
      for (const candidate of candidates) {
        try {
          const chunks = await extractDocumentChunks(candidate.canonicalPath, candidate.extension);
          for (const chunk of chunks) {
            const embedding = await createEmbedding(chunk.content);
            await invoke("store_document_chunk", {
              input: {
                folderId: folder.id,
                documentId: candidate.documentId,
                content: chunk.content,
                chunkIndex: chunk.chunk_index,
                tokenCount: chunk.token_count,
                embedding,
              },
            });
          }
          await invoke("finish_document_indexing", { documentId: candidate.documentId, error: null });
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          await invoke("finish_document_indexing", { documentId: candidate.documentId, error: message });
          scanError = scanError ?? `Algunos documentos no pudieron analizarse: ${message}`;
        }
      }
    } catch (error) {
      scanError = error instanceof Error ? error.message : String(error);
    }
    try {
      const updated = await invoke<Folder>("finish_folder_scan", {
        folderId: folder.id,
        error: scanError,
      });
      replaceFolder(updated);
    } catch (error) {
      replaceFolder({ ...folder, scanStatus: "failed", lastError: String(error) });
    }
  };

  useEffect(() => {
    if (!inTauri() || startupScanStarted.current || folders.length === 0) return;
    startupScanStarted.current = true;
    void (async () => {
      for (const folder of folders) {
        await scanFolder(folder);
      }
    })();
  }, [folders]);

  const saveFolderConfiguration = async () => {
    if (!folderDraft) return;
    setFolderBusy(true);
    setFolderError("");
    try {
      const existing = folderDraft.id
        ? folders.find((folder) => folder.id === folderDraft.id)
        : undefined;
      const folder = folderDraft.id
        ? await invoke<Folder>("update_folder", {
            input: {
              id: folderDraft.id,
              name: folderDraft.name,
              canonicalPath: folderDraft.path,
              lastScannedAt: existing?.lastScannedAt ?? null,
              scanStatus: "pending",
              lastError: null,
              extensions: folderExtensions,
            },
          })
        : await invoke<Folder>("create_folder", {
            input: {
              name: folderDraft.name,
              canonicalPath: folderDraft.path,
              extensions: folderExtensions,
            },
          });
      replaceFolder(folder);
      setFolderDraft(null);
      setFolderModal(false);
      await openChat(folder.id);
      void scanFolder(folder);
    } catch (error) {
      setFolderError(String(error));
    } finally {
      setFolderBusy(false);
    }
  };

  const deleteFolder = async (id: number) => {
    if (inTauri()) {
      try { await invoke("delete_folder", { id }); } catch (error) { setFolderError(String(error)); return; }
    }
    setFolders((current) => current.filter((folder) => folder.id !== id));
    setFolderModal(false);
    setFolderDraft(null);
    setConfirmFolderDelete(false);
    if (selectedFolder === id) {
      selectedFolderRef.current = null;
      setSelectedFolder(null);
      setMessages([]);
      setView("chat");
    }
  };

  const attachDocument = async () => {
    if (!inTauri()) {
      setAttachmentError("Los archivos se pueden adjuntar desde la aplicación de escritorio.");
      return;
    }
    setAttachmentError("");
    try {
      const selected = await open({ multiple: false, directory: false, title: "Adjuntar un documento" });
      if (!selected) return;
      if (selectedFolder !== null) {
        setPendingAttachment(selected);
        return;
      }
      setAttachmentBusy(true);
      const document = await invoke<DocumentRecord>("attach_document", { filePath: selected, folderId: null });
      setDocuments([document]);
    } catch (error) {
      setAttachmentError(String(error));
    } finally {
      setAttachmentBusy(false);
    }
  };

  const confirmFolderAttachment = async () => {
    if (!pendingAttachment || selectedFolder === null) return;
    const targetFolder = folders.find((folder) => folder.id === selectedFolder);
    setAttachmentBusy(true);
    setAttachmentError("");
    try {
      const document = await invoke<DocumentRecord>("attach_document", {
        filePath: pendingAttachment,
        folderId: selectedFolder,
      });
      setDocuments([document]);
      setPendingAttachment(null);
      if (targetFolder) void scanFolder(targetFolder);
    } catch (error) {
      setAttachmentError(String(error));
    } finally {
      setAttachmentBusy(false);
    }
  };

  return (
    <Box className="app-shell">
      <aside className="sidebar">
        <Group className="brand-row">
          <Group gap={10}>
            <Box className="brand-mark">M</Box>
            <Text fw={750} size="lg" className="brand-name">MEMORIÓN</Text>
          </Group>
          <Tooltip label="Configuración">
            <ActionIcon variant="subtle" color="gray" aria-label="Configuración" onClick={() => setSettingsModal(true)}>
              <IconSettings size={19} />
            </ActionIcon>
          </Tooltip>
        </Group>

        <Stack gap={4} mt="lg">
          <button className={`nav-item ${view === "chat" && selectedFolder === null ? "active" : ""}`} onClick={() => openChat(null)}>
            <IconMessage size={18} /><span>Chat general</span>
          </button>
          <button className={`nav-item ${view === "analytics" ? "active" : ""}`} onClick={() => setView("analytics")}>
            <IconChartBar size={18} /><span>Actividad</span>
          </button>
        </Stack>

        <Group justify="space-between" mt="xl" mb={7} px={8}>
          <Text size="xs" fw={700} c="dimmed" tt="uppercase" lts={1.1}>Carpetas</Text>
          <Group gap={2}>
            {editingFolders && <Tooltip label="Crear chat de carpeta"><ActionIcon variant="subtle" color="gray" size="sm" aria-label="Crear chat de carpeta" onClick={selectFolder}><IconPlus size={16} /></ActionIcon></Tooltip>}
            <Tooltip label={editingFolders ? "Terminar edición" : "Editar configuraciones"}>
              <ActionIcon variant={editingFolders ? "light" : "subtle"} color={editingFolders ? "violet" : "gray"} size="sm" aria-label="Editar configuraciones" onClick={() => setEditingFolders((value) => !value)}>
                <IconEdit size={16} />
              </ActionIcon>
            </Tooltip>
          </Group>
        </Group>
        <TextInput
          value={search}
          onChange={(event) => setSearch(event.currentTarget.value)}
          leftSection={<IconSearch size={15} />}
          placeholder="Buscar carpeta"
          aria-label="Buscar carpetas"
          className="search-input"
        />
        <ScrollArea className="folder-scroll">
          <Stack gap={3}>
            {folders.filter((folder) => folder.name.toLowerCase().includes(search.toLowerCase())).map((folder) => (
              <div key={folder.id} className={`folder-row ${selectedFolder === folder.id ? "active" : ""}`}>
                <button className="folder-item" onClick={() => openChat(folder.id)}>
                  <IconFolder size={17} color="var(--mantine-color-violet-4)" />
                  <span>{folder.name}</span>
                  <span className={`folder-status ${folder.scanStatus}`} title={scanLabel(folder.scanStatus)} />
                </button>
                {editingFolders && <Group gap={0} wrap="nowrap" className="folder-row-actions">
                  <Tooltip label={`Reindexar ${folder.name}`}><ActionIcon variant="subtle" color="gray" size="sm" aria-label={`Reindexar ${folder.name}`} loading={folder.scanStatus === "scanning"} onClick={() => void scanFolder(folder)}><IconRefresh size={15} /></ActionIcon></Tooltip>
                  <Tooltip label={`Modificar ${folder.name}`}><ActionIcon variant="subtle" color="gray" size="sm" aria-label={`Modificar ${folder.name}`} onClick={() => editFolderConfiguration(folder)}><IconSettings size={15} /></ActionIcon></Tooltip>
                </Group>}
              </div>
            ))}
          </Stack>
        </ScrollArea>

      </aside>

      <main className="main-panel">
        <header className="topbar">
          <Group gap="xs">
            {view === "chat" && <><Text fw={650}>{activeFolder ? `Chat · ${activeFolder.name}` : "Chat general"}</Text>{activeFolder ? <Tooltip label={activeFolder.lastError || `Estado del análisis: ${scanLabel(activeFolder.scanStatus)}`} multiline w={280}><Badge variant="light" color={scanColor(activeFolder.scanStatus)} size="sm">{scanLabel(activeFolder.scanStatus)}</Badge></Tooltip> : <Badge variant="light" color="gray" size="sm">Sin carpeta</Badge>}</>}
            {view === "analytics" && <Text fw={650}>Actividad y uso</Text>}
          </Group>
        </header>

        {view === "chat" && (
          <section className="chat-view">
            {messages.length === 0 ? (
              <Center className="welcome-wrap">
                <Stack align="center" gap="lg" w="100%">
                  <Stack align="center" gap={5}>
                    <Title order={1}>Hola, André.</Title>
                    <Text c="dimmed" size="lg">Tu espacio para pensar, crear y recordar.</Text>
                  </Stack>
                </Stack>
              </Center>
            ) : (
              <ScrollArea className="messages-area">
                <Stack gap="xl" maw={760} mx="auto" py="xl">
                  {messages.map((message, index) => (
                    <Group key={index} align="flex-start" wrap="nowrap" className={`message ${message.role}`}>
                      <Avatar size={34} radius="xl" color={message.role === "assistant" ? "violet" : "gray"}>
                        {message.role === "assistant" ? <IconMessage size={17} /> : <IconUser size={17} />}
                      </Avatar>
                      <Paper className="message-bubble">
                        {message.role === "assistant"
                          ? <MarkdownMessage content={message.content} />
                          : <Text size="sm" lh={1.7}>{message.content}</Text>}
                      </Paper>
                    </Group>
                  ))}
                  {chatBusy && (
                    <Group align="flex-start" wrap="nowrap" className="message assistant">
                      <Avatar size={34} radius="xl" color="violet"><IconMessage size={17} /></Avatar>
                      <Paper className="message-bubble">
                        <Group gap="xs"><Loader size="xs" color="violet" /><Text size="sm" c="dimmed">{busyChatFolderId === selectedFolder ? "Pensando…" : "Pensando en otro chat…"}</Text></Group>
                      </Paper>
                    </Group>
                  )}
                </Stack>
              </ScrollArea>
            )}
            <Composer prompt={prompt} setPrompt={setPrompt} submitPrompt={submitPrompt}
              documents={documents} attachDocument={attachDocument}
              pendingAttachment={pendingAttachment} confirmFolderAttachment={confirmFolderAttachment}
              cancelFolderAttachment={() => setPendingAttachment(null)}
              attachmentBusy={attachmentBusy} attachmentError={attachmentError}
              chatBusy={chatBusy} chatError={chatError} />
          </section>
        )}

        {view === "analytics" && <Analytics />}
      </main>

      <Modal opened={folderModal} onClose={() => { setFolderModal(false); setFolderDraft(null); setConfirmFolderDelete(false); }} title={folderDraft?.id ? "Modificar chat de carpeta" : "Nuevo chat de carpeta"} centered overlayProps={{ backgroundOpacity: 0.65, blur: 5 }}>
        {folderDraft && <Stack gap="md">
          <div><Text fw={650}>{folderDraft.name}</Text><Text size="xs" c="dimmed" truncate>{folderDraft.path}</Text></div>
          <Paper p="md" className="folder-scan-notice">
            <Text size="sm" fw={600}>Formatos que se indexarán</Text>
            <Text size="xs" c="dimmed" mt={4}>El escaneo incluye subcarpetas y omite enlaces simbólicos.</Text>
            <SimpleGrid cols={2} spacing="xs" mt="md">
              {SUPPORTED_DOCUMENT_FORMATS.map((format) => {
                const extension = format.slice(1);
                return <Checkbox key={format} label={format} checked={folderExtensions.includes(extension)}
                  onChange={(event) => setFolderExtensions((current) => event.currentTarget.checked
                    ? [...new Set([...current, extension])]
                    : current.filter((item) => item !== extension))} />;
              })}
            </SimpleGrid>
            <Text size="xs" c="dimmed" mt="sm">CSV y XLSX todavía no se indexan.</Text>
          </Paper>
          {folderDraft.id && <Paper p="md" className="folder-delete-zone">
            <Text size="sm" fw={600} c="red.4">Eliminar este chat</Text>
            <Text size="xs" c="dimmed" mt={3}>Se eliminarán sus documentos registrados, historial de sesión, conocimientos y embeddings. Los archivos físicos no se borrarán.</Text>
            {confirmFolderDelete
              ? <Group gap="xs" mt="sm"><Button size="xs" color="red" onClick={() => deleteFolder(folderDraft.id!)}>Confirmar eliminación</Button><Button size="xs" variant="subtle" color="gray" onClick={() => setConfirmFolderDelete(false)}>Cancelar</Button></Group>
              : <Button size="xs" variant="light" color="red" leftSection={<IconTrash size={15} />} mt="sm" onClick={() => setConfirmFolderDelete(true)}>Eliminar chat</Button>}
          </Paper>}
        </Stack>}
        {folderError && <Text size="sm" c="red.4" mt="md">{folderError}</Text>}
        <Group justify="space-between" mt="xl">
          <Button variant="subtle" color="gray" onClick={() => { setFolderModal(false); setFolderDraft(null); }}>Cancelar</Button>
          <Button onClick={saveFolderConfiguration} loading={folderBusy}>{folderDraft?.id ? "Guardar y reindexar" : "Crear e indexar"}</Button>
        </Group>
      </Modal>
      <Modal opened={settingsModal} onClose={() => setSettingsModal(false)} title="Configuración" centered size="sm" overlayProps={{ backgroundOpacity: 0.65, blur: 5 }}>
        <Button variant="light" color="violet" fullWidth justify="flex-start" leftSection={<IconFlag size={18} />} onClick={reportIssue}>
          Reportar un problema
        </Button>
        <Text size="xs" c="dimmed" mt="sm">Abre GitHub para informar errores o comportamientos extraños.</Text>
      </Modal>
    </Box>
  );
}

function Composer({ prompt, setPrompt, submitPrompt, documents, attachDocument, pendingAttachment, confirmFolderAttachment, cancelFolderAttachment, attachmentBusy, attachmentError, chatBusy, chatError }: {
  prompt: string;
  setPrompt: (value: string) => void;
  submitPrompt: () => void;
  documents: DocumentRecord[];
  attachDocument: () => void;
  pendingAttachment: string | null;
  confirmFolderAttachment: () => void;
  cancelFolderAttachment: () => void;
  attachmentBusy: boolean;
  attachmentError: string;
  chatBusy: boolean;
  chatError: string;
}) {
  return (
    <div className="composer-wrap">
      <Paper className="composer" shadow="xl">
        {pendingAttachment && (
          <Paper className="attachment-confirmation" mb="xs">
            <Text size="sm" fw={600}>Esto copiará este archivo al folder vinculado a este chat, ¿está de acuerdo?</Text>
            <Text size="xs" c="dimmed" truncate mt={2}>{pendingAttachment.split(/[\\/]/).pop()}</Text>
            <Group gap="xs" mt="sm">
              <Button size="xs" onClick={confirmFolderAttachment} loading={attachmentBusy}>Sí, copiar</Button>
              <Button size="xs" variant="subtle" color="gray" onClick={cancelFolderAttachment} disabled={attachmentBusy}>Cancelar</Button>
            </Group>
          </Paper>
        )}
        {documents.length > 0 && <Group gap={6} mb="xs" className="attachment-list">
          {documents.map((document) => <Paper key={document.id} className="attachment-chip">
            <IconFileText size={14} /><Text size="xs" truncate>{document.relativePath}</Text>
          </Paper>)}
        </Group>}
        <Textarea value={prompt} onChange={(event) => setPrompt(event.currentTarget.value)} onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); submitPrompt(); } }} placeholder="Pregunta o pide recordar algo..." aria-label="Mensaje" autosize minRows={1} maxRows={4} variant="unstyled" disabled={chatBusy} />
        <Group justify="space-between" mt="xs"><ActionIcon variant="subtle" color="gray" aria-label="Adjuntar archivo" onClick={attachDocument} loading={attachmentBusy}><IconPlus size={19} /></ActionIcon><ActionIcon size="lg" radius="md" color="violet" aria-label="Enviar mensaje" onClick={submitPrompt} loading={chatBusy} disabled={!prompt.trim()}><IconSend size={18} /></ActionIcon></Group>
      </Paper>
      {attachmentError && <Text ta="center" size="xs" c="red.4" mt={7}>{attachmentError}</Text>}
      {chatError && <Text ta="center" size="xs" c="red.4" mt={7}>{chatError}</Text>}
      <Text ta="center" size="xs" c="dimmed" mt={9}>MEMORIÓN puede cometer errores. Verifica la información importante.</Text>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let index = 1; value >= 1024 && index < units.length; index += 1) {
    value /= 1024;
    unit = units[index];
  }
  return `${value.toFixed(value >= 10 ? 1 : 2)} ${unit}`;
}

function Analytics() {
  const [cpu, setCpu] = useState<number[]>(Array(12).fill(0));
  const [ram, setRam] = useState<number[]>(Array(12).fill(0));
  const [metrics, setMetrics] = useState<SystemMetrics>({ cpuPercent: 0, ramUsedBytes: 0, ramTotalBytes: 0 });
  const [activity, setActivity] = useState<ActivityMetrics | null>(null);
  const [activityError, setActivityError] = useState("");
  useEffect(() => {
    if (!inTauri()) return;
    invoke<ActivityMetrics>("get_activity_metrics")
      .then(setActivity)
      .catch((error) => setActivityError(String(error)));
    const updateMetrics = async () => {
      try {
        const next = await invoke<SystemMetrics>("get_system_metrics");
        setMetrics(next);
        setCpu((values) => [...values.slice(1), Math.min(100, Math.max(0, next.cpuPercent))]);
        const ramPercent = next.ramTotalBytes ? (next.ramUsedBytes / next.ramTotalBytes) * 100 : 0;
        setRam((values) => [...values.slice(1), Math.min(100, Math.max(0, ramPercent))]);
      } catch (error) { console.error(error); }
    };
    updateMetrics();
    const timer = window.setInterval(updateMetrics, 2000);
    return () => window.clearInterval(timer);
  }, []);
  const usedRamGb = metrics.ramUsedBytes / 1024 ** 3;
  const totalRamGb = metrics.ramTotalBytes / 1024 ** 3;
  const cards = [
    { label: "Chats de carpeta", value: activity ? String(activity.folderChatCount) : "—", delta: "Registrados", icon: IconMessage, tooltip: undefined },
    { label: "Mensajes en esta sesión", value: activity ? String(activity.sessionMessageCount) : "—", delta: "Temporal", icon: IconActivity, tooltip: undefined },
    {
      label: "Peso del texto de la sesión",
      value: activity ? formatBytes(activity.sessionTextBytes) : "—",
      delta: "UTF-8",
      icon: IconFileText,
      tooltip: "Suma de los bytes UTF-8 de los mensajes temporales. No representa la RAM consumida por la aplicación.",
    },
  ];
  return (
    <section className="analytics-view"><div className="analytics-container">
      <div className="analytics-heading"><Text size="sm" c="violet.3" fw={650}>Estado de la sesión actual</Text><Title order={2} mt={2}>Tu actividad</Title><Text c="dimmed" size="sm" mt={3}>Datos locales medidos directamente por MEMORIÓN.</Text></div>
      <SimpleGrid cols={3} spacing="md" className="metrics-grid">
        {cards.map((metric) => <Card key={metric.label} className="metric-card"><Group justify="space-between"><ThemeIcon variant="light" color="violet" size={34}><metric.icon size={18} /></ThemeIcon>{metric.tooltip ? <Tooltip label={metric.tooltip} multiline w={280}><Badge className="metric-help-badge" variant="light" color="teal">{metric.delta}</Badge></Tooltip> : <Badge variant="light" color="teal">{metric.delta}</Badge>}</Group><Text size="xl" fw={750} mt="sm">{metric.value}</Text><Text size="sm" c="dimmed">{metric.label}</Text></Card>)}
      </SimpleGrid>
      <div className="analytics-charts">
        <Card className="chart-card activity-chart">
          <Group justify="space-between"><div><Text fw={700}>Contenido de carpetas mapeadas</Text><Text size="sm" c="dimmed">Suma real de los archivos accesibles</Text></div><ThemeIcon variant="light" color="violet" size={36}><IconFolder size={19} /></ThemeIcon></Group>
          <Stack justify="center" gap="lg" className="mapped-storage">
            <div><Text fz={34} fw={760}>{activity ? formatBytes(activity.mappedFolderBytes) : "—"}</Text><Text size="sm" c="dimmed">Tamaño total en disco</Text></div>
            <Group grow>
              <Paper className="storage-detail">
                <Text fw={700}>{activity?.mappedFileCount ?? "—"}</Text>
                <Group gap={4} wrap="nowrap">
                  <Text size="xs" c="dimmed">Archivos encontrados</Text>
                  <Tooltip label="Todos los archivos descubiertos recursivamente dentro de las carpetas registradas." multiline w={240}>
                    <IconInfoCircle className="metric-info" size={14} />
                  </Tooltip>
                </Group>
              </Paper>
              <Paper className="storage-detail">
                <Text fw={700}>{activity?.mappedDirectoryCount ?? "—"}</Text>
                <Group gap={4} wrap="nowrap">
                  <Text size="xs" c="dimmed">Carpetas encontradas</Text>
                  <Tooltip label="Total de carpetas encontradas: incluye las carpetas raíz registradas como chats y todas sus subcarpetas accesibles." multiline w={260}>
                    <IconInfoCircle className="metric-info" size={14} />
                  </Tooltip>
                </Group>
              </Paper>
            </Group>
            {activity && activity.inaccessibleEntryCount > 0 && <Text size="xs" c="yellow.5">{activity.inaccessibleEntryCount} rutas no pudieron leerse.</Text>}
            {activityError && <Text size="xs" c="red.4">{activityError}</Text>}
          </Stack>
        </Card>
        <div className="resource-stack"><ResourceCard title="Consumo de CPU" value={`${metrics.cpuPercent.toFixed(0)}%`} data={cpu} color="#8b5cf6" /><ResourceCard title="Consumo de RAM" value={`${usedRamGb.toFixed(1)} GB`} subtitle={`de ${totalRamGb.toFixed(1)} GB`} data={ram} color="#2dd4bf" /></div>
      </div>
    </div></section>
  );
}

function ResourceCard({ title, value, subtitle, data, color }: { title: string; value: string; subtitle?: string; data: number[]; color: string }) {
  const points = data.map((item, index) => `${(index / (data.length - 1)) * 100},${100 - item}`).join(" ");
  return <Card className="resource-card"><Group justify="space-between" align="flex-start"><div><Text size="sm" fw={650}>{title}</Text><Group gap={6} align="baseline" mt={3}><Text fz={25} fw={750}>{value}</Text>{subtitle && <Text size="xs" c="dimmed">{subtitle}</Text>}</Group></div><Badge variant="dot" color="teal">En vivo</Badge></Group><div className="resource-line"><svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-label={`Historial de ${title}`}><polyline points={points} fill="none" stroke={color} strokeWidth="3" vectorEffect="non-scaling-stroke" /></svg></div><Text size="xs" c="dimmed">12 muestras recientes · actualización cada 2 s</Text></Card>;
}

export default App;
