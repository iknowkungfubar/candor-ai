import { useState, useEffect, useRef, type KeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

// ─── Types ──────────────────────────────────────────────────────────────────

interface Health {
  status: string;
  version?: string;
  subsystems?: Record<string, unknown>;
}

interface Status {
  session_id?: string;
  current_phase?: string;
  features?: string[];
}

interface TaskEvent {
  type: string;
  content?: string;
  done?: boolean;
  error?: string;
}

// ─── App ─────────────────────────────────────────────────────────────────────

function App() {
  const [health, setHealth] = useState<Health | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [taskInput, setTaskInput] = useState("");
  const [isaId, setIsaId] = useState("");
  const [logs, setLogs] = useState<string[]>([]);
  const [running, setRunning] = useState(false);
  const logEndRef = useRef<HTMLDivElement>(null);

  // ── Poll health & status ───────────────────────────────────────────────

  useEffect(() => {
    const fetchStatus = async () => {
      try {
        const h = await invoke<Health>("get_daemon_health");
        setHealth(h);
        setError(null);
      } catch (e) {
        setHealth(null);
        setError(String(e));
      }
      try {
        const s = await invoke<Status>("get_daemon_status");
        setStatus(s);
      } catch {
        // non-critical
      }
    };

    fetchStatus();
    const interval = setInterval(fetchStatus, 5000);
    return () => clearInterval(interval);
  }, []);

  // ── Auto-scroll logs ───────────────────────────────────────────────────

  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  // ── Submit task ────────────────────────────────────────────────────────

  const handleSubmit = async () => {
    if (!taskInput.trim()) return;

    setRunning(true);
    setLogs((prev) => [...prev, `> ${taskInput}`]);

    try {
      const events = await invoke<TaskEvent[]>("send_task", {
        description: taskInput.trim(),
        isaId: isaId.trim() || null,
      });

      for (const event of events) {
        if (event.type === "log" && event.content) {
          setLogs((prev) => [...prev, event.content!]);
        }
        if (event.type === "error" && event.content) {
          setLogs((prev) => [...prev, `[ERROR] ${event.content!}`]);
        }
      }
    } catch (e) {
      setLogs((prev) => [...prev, `[ERROR] ${String(e)}`]);
    } finally {
      setRunning(false);
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      handleSubmit();
    }
  };

  // ── Connected? ─────────────────────────────────────────────────────────

  const connected = health !== null && health.status === "ok";

  return (
    <div className="app">
      {/* ── Header ──────────────────────────────────────────────────── */}
      <header className="header">
        <h1>Candor AI</h1>
        <div className="status-badge" data-connected={connected}>
          <span className="dot" />
          {connected ? "Connected" : "Disconnected"}
        </div>
      </header>

      {/* ── Panels ──────────────────────────────────────────────────── */}
      <div className="panels">
        {/* Health panel */}
        <div className="panel health-panel">
          <h2>Health</h2>
          {error && !health && <p className="error">{error}</p>}
          {health ? (
            <ul>
              <li><strong>Status:</strong> {health.status}</li>
              <li><strong>Version:</strong> {health.version || "unknown"}</li>
              <li>
                <strong>Subsystems:</strong>{" "}
                {health.subsystems
                  ? Object.entries(health.subsystems)
                      .map(([k, v]) => `${k}=${JSON.stringify(v)}`)
                      .join(", ")
                  : "none"}
              </li>
            </ul>
          ) : (
            <p className="dim">Waiting for daemon…</p>
          )}
        </div>

        {/* Status panel */}
        <div className="panel status-panel">
          <h2>Status</h2>
          {status ? (
            <ul>
              <li>
                <strong>Session:</strong>{" "}
                {status.session_id || "—"}
              </li>
              <li>
                <strong>Phase:</strong>{" "}
                {status.current_phase || "idle"}
              </li>
              <li>
                <strong>Features:</strong>{" "}
                {status.features?.join(", ") || "—"}
              </li>
            </ul>
          ) : (
            <p className="dim">No status data</p>
          )}
        </div>
      </div>

      {/* ── Task input ──────────────────────────────────────────────── */}
      <div className="task-input-area">
        <input
          type="text"
          placeholder="Describe a task for the agent…"
          value={taskInput}
          onChange={(e) => setTaskInput(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={running}
        />
        <input
          type="text"
          placeholder="ISA ID (optional)"
          value={isaId}
          onChange={(e) => setIsaId(e.target.value)}
          disabled={running}
          className="isa-input"
        />
        <button onClick={handleSubmit} disabled={running || !taskInput.trim()}>
          {running ? "Running…" : "Submit"}
        </button>
      </div>

      {/* ── Log output ─────────────────────────────────────────────── */}
      <div className="log-panel">
        <h2>Agent Log</h2>
        <div className="log-content">
          {logs.length === 0 && (
            <p className="dim">Submit a task to see agent output here.</p>
          )}
          {logs.map((line, i) => (
            <pre key={i} className="log-line">
              {line}
            </pre>
          ))}
          <div ref={logEndRef} />
        </div>
        {logs.length > 0 && (
          <button
            className="clear-btn"
            onClick={() => setLogs([])}
          >
            Clear
          </button>
        )}
      </div>
    </div>
  );
}

export default App;
