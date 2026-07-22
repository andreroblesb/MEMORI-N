import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ActionIcon,
  Avatar,
  Badge,
  Box,
  Button,
  Card,
  Center,
  Group,
  Menu,
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
  IconBook2,
  IconBulb,
  IconChartBar,
  IconChevronDown,
  IconDots,
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
} from "@tabler/icons-react";
import "./App.css";

type View = "chat" | "library" | "analytics";
type Folder = { id: number; name: string; count: number; color: string };
type Message = { role: "user" | "assistant"; content: string };
type SystemMetrics = { cpuPercent: number; ramUsedBytes: number; ramTotalBytes: number };

const inTauri = () => "__TAURI_INTERNALS__" in window;

const starterFolders: Folder[] = [
  { id: 1, name: "Investigación", count: 12, color: "violet" },
  { id: 2, name: "Universidad", count: 8, color: "blue" },
  { id: 3, name: "Ideas de producto", count: 5, color: "teal" },
];

const recentChats = [
  { title: "Marco teórico de la tesis", meta: "Investigación · hace 12 min", icon: IconBook2 },
  { title: "Resumen de redes neuronales", meta: "Universidad · ayer", icon: IconFileText },
  { title: "Naming para la aplicación", meta: "Ideas de producto · 18 jul", icon: IconBulb },
];

function App() {
  const [view, setView] = useState<View>("chat");
  const [folders, setFolders] = useState(starterFolders);
  const [selectedFolder, setSelectedFolder] = useState<number | null>(null);
  const [folderModal, setFolderModal] = useState(false);
  const [newFolder, setNewFolder] = useState("");
  const [prompt, setPrompt] = useState("");
  const [messages, setMessages] = useState<Message[]>([]);
  const [search, setSearch] = useState("");

  const activeFolder = useMemo(
    () => folders.find((folder) => folder.id === selectedFolder),
    [folders, selectedFolder],
  );

  useEffect(() => {
    if (!inTauri()) return;
    invoke<Folder[]>("list_folder_chats").then(setFolders).catch(console.error);
  }, []);

  const openChat = async (folderId: number | null) => {
    setSelectedFolder(folderId);
    setView("chat");
    if (!inTauri()) {
      setMessages([]);
      return;
    }
    try {
      setMessages(await invoke<Message[]>("get_messages", { folderId }));
    } catch (error) {
      console.error(error);
      setMessages([]);
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
        setFolders(await invoke<Folder[]>("list_folder_chats"));
        return;
      } catch (error) {
        console.error(error);
      }
    }
    setMessages((current) => [...current, { role: "user", content: clean }, { role: "assistant", content: "Mensaje recibido en el modo de demostración del frontend." }]);
  };

  const createFolder = async () => {
    const name = newFolder.trim();
    if (!name) return;
    if (inTauri()) {
      try {
        const folder = await invoke<Folder>("create_folder_chat", { name });
        setFolders((current) => [...current, folder]);
      } catch (error) { console.error(error); return; }
    } else {
      const id = Date.now();
      setFolders((current) => [...current, { id, name, count: 0, color: "grape" }]);
    }
    setNewFolder("");
  };

  const deleteFolder = async (id: number) => {
    if (inTauri()) {
      try { await invoke("delete_folder_chat", { id }); } catch (error) { console.error(error); return; }
    }
    setFolders((current) => current.filter((folder) => folder.id !== id));
    if (selectedFolder === id) {
      setSelectedFolder(null);
      setMessages([]);
      setView("chat");
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
                <IconFolder size={17} color={`var(--mantine-color-${folder.color}-4)`} />
                <span>{folder.name}</span>
                <span className="folder-count">{folder.count}</span>
              </button>
            ))}
          </Stack>
        </ScrollArea>

      </aside>

      <main className="main-panel">
        <header className="topbar">
          <Group gap="xs">
            {view === "chat" && <><Text fw={650}>{activeFolder ? `Chat · ${activeFolder.name}` : "Chat general"}</Text>{!activeFolder && <Badge variant="light" color="gray" size="sm">Sin carpeta</Badge>}</>}
            {view === "library" && <Text fw={650}>{activeFolder?.name ?? "Carpetas"}</Text>}
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
            <Composer prompt={prompt} setPrompt={setPrompt} submitPrompt={submitPrompt} />
          </section>
        )}

        {view === "library" && (
          <ScrollArea className="content-scroll">
            <div className="content-container">
              <Group justify="space-between" align="flex-end" mb="xl">
                <div>
                  <Text size="sm" c="violet.3" fw={650}>{activeFolder ? "Colección" : "Tu biblioteca"}</Text>
                  <Title order={2} mt={4}>{activeFolder?.name ?? "Organiza tus conversaciones"}</Title>
                  <Text c="dimmed" mt={6}>{activeFolder ? `${activeFolder.count} conversaciones guardadas` : "Crea carpetas para mantener cada idea en su lugar."}</Text>
                </div>
                <Button leftSection={<IconEdit size={17} />} onClick={() => setFolderModal(true)}>Editar chats</Button>
              </Group>
              {!activeFolder && (
                <SimpleGrid cols={{ base: 1, md: 3 }} spacing="md" mb={38}>
                  {folders.map((folder) => (
                    <Card key={folder.id} className="folder-card" onClick={() => setSelectedFolder(folder.id)}>
                      <Group justify="space-between"><ThemeIcon color={folder.color} variant="light" size={42} radius="md"><IconFolder size={22} /></ThemeIcon>
                        <Menu position="bottom-end"><Menu.Target><ActionIcon variant="subtle" color="gray" aria-label={`Opciones de ${folder.name}`} onClick={(event) => event.stopPropagation()}><IconDots size={18} /></ActionIcon></Menu.Target><Menu.Dropdown><Menu.Item color="red" leftSection={<IconTrash size={15} />} onClick={() => deleteFolder(folder.id)}>Eliminar carpeta</Menu.Item></Menu.Dropdown></Menu>
                      </Group>
                      <Text fw={700} mt="lg">{folder.name}</Text><Text size="sm" c="dimmed">{folder.count} conversaciones</Text>
                    </Card>
                  ))}
                </SimpleGrid>
              )}
              <Group justify="space-between" mb="md"><Title order={4}>Conversaciones recientes</Title><Text size="sm" c="dimmed">Actualizado hoy</Text></Group>
              <Stack gap="sm">
                {recentChats.map((chat) => (
                  <Paper key={chat.title} className="chat-row" onClick={() => setView("chat")}>
                    <ThemeIcon variant="light" color="gray" radius="md" size={40}><chat.icon size={19} /></ThemeIcon>
                    <div className="chat-row-copy"><Text fw={600}>{chat.title}</Text><Text size="xs" c="dimmed">{chat.meta}</Text></div>
                    <Badge variant="dot" color="green">Listo</Badge><IconChevronDown size={16} className="row-chevron" />
                  </Paper>
                ))}
              </Stack>
            </div>
          </ScrollArea>
        )}

        {view === "analytics" && <Analytics />}
      </main>

      <Modal opened={folderModal} onClose={() => setFolderModal(false)} title="Editar chats de carpeta" centered overlayProps={{ backgroundOpacity: 0.65, blur: 5 }}>
        <Text size="sm" c="dimmed" mb="md">Crea un chat asociado a una carpeta específica o elimina uno existente.</Text>
        <Stack gap={8} className="edit-folder-list">
          {folders.map((folder) => (
            <Paper key={folder.id} className="edit-folder-row">
              <Group justify="space-between" wrap="nowrap">
                <Group gap="sm" wrap="nowrap"><ThemeIcon variant="light" color={folder.color} size={32}><IconFolder size={17} /></ThemeIcon><div><Text size="sm" fw={600}>{folder.name}</Text><Text size="xs" c="dimmed">{folder.count} conversaciones</Text></div></Group>
                <Tooltip label={`Eliminar ${folder.name}`}><ActionIcon variant="subtle" color="red" aria-label={`Eliminar ${folder.name}`} onClick={() => deleteFolder(folder.id)}><IconTrash size={17} /></ActionIcon></Tooltip>
              </Group>
            </Paper>
          ))}
        </Stack>
        <TextInput mt="lg" label="Nuevo chat de carpeta" placeholder="Ej. Documentos del proyecto" value={newFolder} onChange={(event) => setNewFolder(event.currentTarget.value)} onKeyDown={(event) => event.key === "Enter" && createFolder()} />
        <Group justify="space-between" mt="xl"><Button variant="subtle" color="gray" onClick={() => setFolderModal(false)}>Cerrar</Button><Button leftSection={<IconPlus size={17} />} onClick={createFolder} disabled={!newFolder.trim()}>Crear chat</Button></Group>
      </Modal>
    </Box>
  );
}

function Composer({ prompt, setPrompt, submitPrompt }: { prompt: string; setPrompt: (value: string) => void; submitPrompt: () => void }) {
  return (
    <div className="composer-wrap">
      <Paper className="composer" shadow="xl">
        <Textarea value={prompt} onChange={(event) => setPrompt(event.currentTarget.value)} onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); submitPrompt(); } }} placeholder="Pregunta, crea o explora una idea..." aria-label="Mensaje" autosize minRows={1} maxRows={4} variant="unstyled" />
        <Group justify="space-between" mt="xs"><ActionIcon variant="subtle" color="gray" aria-label="Adjuntar archivo"><IconPlus size={19} /></ActionIcon><ActionIcon size="lg" radius="md" color="violet" aria-label="Enviar mensaje" onClick={submitPrompt} disabled={!prompt.trim()}><IconSend size={18} /></ActionIcon></Group>
      </Paper>
      <Text ta="center" size="xs" c="dimmed" mt={9}>MEMORIÓN puede cometer errores. Verifica la información importante.</Text>
    </div>
  );
}

function Analytics() {
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
        {[{ label: "Conversaciones", value: "48", delta: "+12%", icon: IconMessage }, { label: "Mensajes", value: "326", delta: "+8%", icon: IconActivity }, { label: "Carpetas activas", value: "7", delta: "+2", icon: IconFolder }].map((metric) => <Card key={metric.label} className="metric-card"><Group justify="space-between"><ThemeIcon variant="light" color="violet" size={34}><metric.icon size={18} /></ThemeIcon><Badge variant="light" color="teal">{metric.delta}</Badge></Group><Text size="xl" fw={750} mt="sm">{metric.value}</Text><Text size="sm" c="dimmed">{metric.label}</Text></Card>)}
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
