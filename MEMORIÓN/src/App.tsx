import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ActionIcon,
  Avatar,
  Badge,
  Box,
  Button,
  Card,
  Center,
  Group,
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
  IconFileText,
  IconFolder,
  IconMessage,
  IconPlus,
  IconSearch,
  IconSend,
  IconSettings,
  IconTrash,
  IconUser,
  IconX,
} from "@tabler/icons-react";
import "./App.css";

type View = "chat" | "analytics";
type Folder = {
  id: number;
  name: string;
  canonicalPath: string;
  scanStatus: string;
  lastError: string | null;
};
type Message = { role: "user" | "assistant"; content: string };
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

const inTauri = () => "__TAURI_INTERNALS__" in window;

function App() {
  const [view, setView] = useState<View>("chat");
  const [folders, setFolders] = useState<Folder[]>([]);
  const [selectedFolder, setSelectedFolder] = useState<number | null>(null);
  const [folderModal, setFolderModal] = useState(false);
  const [folderBusy, setFolderBusy] = useState(false);
  const [folderError, setFolderError] = useState("");
  const [prompt, setPrompt] = useState("");
  const [messages, setMessages] = useState<Message[]>([]);
  const [documents, setDocuments] = useState<DocumentRecord[]>([]);
  const [attachmentBusy, setAttachmentBusy] = useState(false);
  const [attachmentError, setAttachmentError] = useState("");
  const [search, setSearch] = useState("");

  const activeFolder = useMemo(
    () => folders.find((folder) => folder.id === selectedFolder),
    [folders, selectedFolder],
  );

  useEffect(() => {
    if (!inTauri()) return;
    Promise.all([
      invoke<Folder[]>("list_folders"),
      invoke<DocumentRecord[]>("list_documents", { scope: "general", folderId: null }),
    ]).then(([nextFolders, nextDocuments]) => {
      setFolders(nextFolders);
      setDocuments(nextDocuments);
    }).catch((error) => setFolderError(String(error)));
  }, []);

  const openChat = async (folderId: number | null) => {
    setSelectedFolder(folderId);
    setView("chat");
    if (!inTauri()) {
      setMessages([]);
      setDocuments([]);
      return;
    }
    try {
      const [nextMessages, nextDocuments] = await Promise.all([
        invoke<Message[]>("get_messages", { folderId }),
        invoke<DocumentRecord[]>("list_documents", {
          scope: folderId === null ? "general" : "folder",
          folderId,
        }),
      ]);
      setMessages(nextMessages);
      setDocuments(nextDocuments);
    } catch (error) {
      console.error(error);
      setMessages([]);
      setDocuments([]);
    }
  };

  const submitPrompt = async (value = prompt) => {
    const clean = value.trim();
    if (!clean) return;
    setPrompt("");
    if (inTauri()) {
      try {
        const result = await invoke<Message[]>("send_message", { folderId: selectedFolder, content: clean });
        setMessages(result);
        return;
      } catch (error) {
        console.error(error);
      }
    }
    setMessages((current) => [...current, { role: "user", content: clean }, { role: "assistant", content: "Mensaje recibido en el modo de demostración del frontend." }]);
  };

  const selectFolder = async () => {
    if (!inTauri()) {
      setFolderError("La selección de carpetas está disponible en la aplicación de escritorio.");
      return;
    }
    setFolderError("");
    setFolderBusy(true);
    try {
      const selected = await open({ directory: true, multiple: false, recursive: true, title: "Selecciona una carpeta para MEMORIÓN" });
      if (!selected) return;
      const normalized = selected.replace(/[\\/]+$/, "");
      const name = normalized.split(/[\\/]/).pop() || normalized;
      const folder = await invoke<Folder>("create_folder", {
        input: { name, canonicalPath: normalized },
      });
      setFolders((current) => [...current, folder].sort((a, b) => a.name.localeCompare(b.name)));
      await openChat(folder.id);
      setFolderModal(false);
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
    if (selectedFolder === id) {
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
    setAttachmentBusy(true);
    try {
      const selected = await open({ multiple: false, directory: false, title: "Adjuntar un documento" });
      if (!selected) return;
      const document = await invoke<DocumentRecord>("attach_document", {
        filePath: selected,
        folderId: selectedFolder,
      });
      setDocuments((current) => current.some((item) => item.id === document.id) ? current : [...current, document]);
    } catch (error) {
      setAttachmentError(String(error));
    } finally {
      setAttachmentBusy(false);
    }
  };

  const removeDocument = async (id: number) => {
    if (inTauri()) {
      try { await invoke("delete_document", { id }); }
      catch (error) { setAttachmentError(String(error)); return; }
    }
    setDocuments((current) => current.filter((document) => document.id !== id));
  };

  return (
    <Box className="app-shell">
      <aside className="sidebar">
        <Group className="brand-row">
          <Group gap={10}>
            <Box className="brand-mark">M</Box>
            <Text fw={750} size="lg" className="brand-name">MEMORIÓN</Text>
          </Group>
        </Group>

        <Stack gap={4} mt="lg">
          <button className={`nav-item ${view === "chat" && selectedFolder === null ? "active" : ""}`} onClick={() => openChat(null)}>
            <IconMessage size={18} /><span>Chat general</span><span className="shortcut">⌘ K</span>
          </button>
          <button className={`nav-item ${view === "analytics" ? "active" : ""}`} onClick={() => setView("analytics")}>
            <IconChartBar size={18} /><span>Actividad</span>
          </button>
        </Stack>

        <Group justify="space-between" mt="xl" mb={7} px={8}>
          <Text size="xs" fw={700} c="dimmed" tt="uppercase" lts={1.1}>Carpetas</Text>
          <Tooltip label="Editar chats de carpeta">
            <ActionIcon variant="subtle" color="gray" size="sm" aria-label="Editar chats de carpeta" onClick={() => setFolderModal(true)}>
              <IconEdit size={16} />
            </ActionIcon>
          </Tooltip>
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
              <button
                key={folder.id}
                className={`folder-item ${selectedFolder === folder.id ? "active" : ""}`}
                onClick={() => openChat(folder.id)}
              >
                <IconFolder size={17} color="var(--mantine-color-violet-4)" />
                <span>{folder.name}</span>
                <span className={`folder-status ${folder.scanStatus}`} title="Pendiente de indexación" />
              </button>
            ))}
          </Stack>
        </ScrollArea>

      </aside>

      <main className="main-panel">
        <header className="topbar">
          <Group gap="xs">
            {view === "chat" && <><Text fw={650}>{activeFolder ? `Chat · ${activeFolder.name}` : "Chat general"}</Text>{!activeFolder && <Badge variant="light" color="gray" size="sm">Sin carpeta</Badge>}</>}
            {view === "analytics" && <Text fw={650}>Actividad y uso</Text>}
          </Group>
          <Group gap="xs">
            <Tooltip label="Configuración"><ActionIcon variant="subtle" color="gray" aria-label="Configuración"><IconSettings size={20} /></ActionIcon></Tooltip>
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
                      <Paper className="message-bubble"><Text size="sm" lh={1.7}>{message.content}</Text></Paper>
                    </Group>
                  ))}
                </Stack>
              </ScrollArea>
            )}
            <Composer prompt={prompt} setPrompt={setPrompt} submitPrompt={submitPrompt}
              documents={documents} attachDocument={attachDocument} removeDocument={removeDocument}
              attachmentBusy={attachmentBusy} attachmentError={attachmentError} />
          </section>
        )}

        {view === "analytics" && <Analytics folderCount={folders.length} />}
      </main>

      <Modal opened={folderModal} onClose={() => setFolderModal(false)} title="Editar carpetas" centered overlayProps={{ backgroundOpacity: 0.65, blur: 5 }}>
        <Text size="sm" c="dimmed" mb="md">Registra una carpeta para crear su chat. El análisis y la indexación se habilitarán en una fase posterior.</Text>
        <Stack gap={8} className="edit-folder-list">
          {folders.length === 0 && <Text size="sm" c="dimmed" ta="center" py="md">Todavía no hay carpetas registradas.</Text>}
          {folders.map((folder) => (
            <Paper key={folder.id} className="edit-folder-row">
              <Group justify="space-between" wrap="nowrap">
                <Group gap="sm" wrap="nowrap" className="folder-detail"><ThemeIcon variant="light" color="violet" size={32}><IconFolder size={17} /></ThemeIcon><div><Text size="sm" fw={600}>{folder.name}</Text><Text size="xs" c="dimmed" truncate>{folder.canonicalPath}</Text><Text size="xs" c="yellow.5">Pendiente de indexación</Text></div></Group>
                <Tooltip label={`Eliminar ${folder.name}`}><ActionIcon variant="subtle" color="red" aria-label={`Eliminar ${folder.name}`} onClick={() => deleteFolder(folder.id)}><IconTrash size={17} /></ActionIcon></Tooltip>
              </Group>
            </Paper>
          ))}
        </Stack>
        {folderError && <Text size="sm" c="red.4" mt="md">{folderError}</Text>}
        <Group justify="space-between" mt="xl"><Button variant="subtle" color="gray" onClick={() => setFolderModal(false)}>Cerrar</Button><Button leftSection={<IconPlus size={17} />} onClick={selectFolder} loading={folderBusy}>Seleccionar carpeta</Button></Group>
      </Modal>
    </Box>
  );
}

function Composer({ prompt, setPrompt, submitPrompt, documents, attachDocument, removeDocument, attachmentBusy, attachmentError }: {
  prompt: string;
  setPrompt: (value: string) => void;
  submitPrompt: () => void;
  documents: DocumentRecord[];
  attachDocument: () => void;
  removeDocument: (id: number) => void;
  attachmentBusy: boolean;
  attachmentError: string;
}) {
  return (
    <div className="composer-wrap">
      <Paper className="composer" shadow="xl">
        {documents.length > 0 && <Group gap={6} mb="xs" className="attachment-list">
          {documents.map((document) => <Paper key={document.id} className="attachment-chip">
            <IconFileText size={14} /><Text size="xs" truncate>{document.relativePath}</Text>
            <ActionIcon variant="transparent" color="gray" size="xs" aria-label={`Quitar ${document.relativePath}`} onClick={() => removeDocument(document.id)}><IconX size={12} /></ActionIcon>
          </Paper>)}
        </Group>}
        <Textarea value={prompt} onChange={(event) => setPrompt(event.currentTarget.value)} onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); submitPrompt(); } }} placeholder="Pregunta, crea o explora una idea..." aria-label="Mensaje" autosize minRows={1} maxRows={4} variant="unstyled" />
        <Group justify="space-between" mt="xs"><ActionIcon variant="subtle" color="gray" aria-label="Adjuntar archivo" onClick={attachDocument} loading={attachmentBusy}><IconPlus size={19} /></ActionIcon><ActionIcon size="lg" radius="md" color="violet" aria-label="Enviar mensaje" onClick={submitPrompt} disabled={!prompt.trim()}><IconSend size={18} /></ActionIcon></Group>
      </Paper>
      {attachmentError && <Text ta="center" size="xs" c="red.4" mt={7}>{attachmentError}</Text>}
      <Text ta="center" size="xs" c="dimmed" mt={9}>MEMORIÓN puede cometer errores. Verifica la información importante.</Text>
    </div>
  );
}

function Analytics({ folderCount }: { folderCount: number }) {
  const bars = [32, 46, 38, 72, 55, 84, 68, 94, 64, 78, 58, 86];
  const [cpu, setCpu] = useState([26, 34, 31, 48, 43, 62, 39, 52, 45, 58, 41, 47]);
  const [ram, setRam] = useState([52, 54, 53, 58, 61, 60, 64, 67, 66, 69, 68, 71]);
  const [metrics, setMetrics] = useState<SystemMetrics>({ cpuPercent: 47, ramUsedBytes: 5.7 * 1024 ** 3, ramTotalBytes: 8 * 1024 ** 3 });
  useEffect(() => {
    if (!inTauri()) return;
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
  return (
    <section className="analytics-view"><div className="analytics-container">
      <div className="analytics-heading"><Text size="sm" c="violet.3" fw={650}>Resumen de los últimos 30 días</Text><Title order={2} mt={2}>Tu actividad</Title><Text c="dimmed" size="sm" mt={3}>Actividad y recursos del equipo en una sola vista.</Text></div>
      <SimpleGrid cols={3} spacing="md" className="metrics-grid">
        {[{ label: "Conversaciones", value: "48", delta: "+12%", icon: IconMessage }, { label: "Mensajes", value: "326", delta: "+8%", icon: IconActivity }, { label: "Carpetas registradas", value: String(folderCount), delta: "Local", icon: IconFolder }].map((metric) => <Card key={metric.label} className="metric-card"><Group justify="space-between"><ThemeIcon variant="light" color="violet" size={34}><metric.icon size={18} /></ThemeIcon><Badge variant="light" color="teal">{metric.delta}</Badge></Group><Text size="xl" fw={750} mt="sm">{metric.value}</Text><Text size="sm" c="dimmed">{metric.label}</Text></Card>)}
      </SimpleGrid>
      <div className="analytics-charts">
        <Card className="chart-card activity-chart"><Group justify="space-between"><div><Text fw={700}>Mensajes por día</Text><Text size="sm" c="dimmed">Actividad reciente</Text></div><Badge variant="outline" color="gray">12 días</Badge></Group><div className="bar-chart">{bars.map((height, index) => <div key={index} className="bar-track"><div className="bar" style={{ height: `${height}%` }} /></div>)}</div><Group justify="space-between"><Text size="xs" c="dimmed">8 jul</Text><Text size="xs" c="dimmed">Hoy</Text></Group></Card>
        <div className="resource-stack"><ResourceCard title="Consumo de CPU" value={`${metrics.cpuPercent.toFixed(0)}%`} data={cpu} color="#8b5cf6" /><ResourceCard title="Consumo de RAM" value={`${usedRamGb.toFixed(1)} GB`} subtitle={`de ${totalRamGb.toFixed(1)} GB`} data={ram} color="#2dd4bf" /></div>
      </div>
    </div></section>
  );
}

function ResourceCard({ title, value, subtitle, data, color }: { title: string; value: string; subtitle?: string; data: number[]; color: string }) {
  const points = data.map((item, index) => `${(index / (data.length - 1)) * 100},${100 - item}`).join(" ");
  return <Card className="resource-card"><Group justify="space-between" align="flex-start"><div><Text size="sm" fw={650}>{title}</Text><Group gap={6} align="baseline" mt={3}><Text fz={25} fw={750}>{value}</Text>{subtitle && <Text size="xs" c="dimmed">{subtitle}</Text>}</Group></div><Badge variant="dot" color="teal">En vivo</Badge></Group><div className="resource-line"><svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-label={`Historial de ${title}`}><polyline points={points} fill="none" stroke={color} strokeWidth="3" vectorEffect="non-scaling-stroke" /></svg></div><Group justify="space-between"><Text size="xs" c="dimmed">-10 min</Text><Text size="xs" c="dimmed">Ahora</Text></Group></Card>;
}

export default App;
