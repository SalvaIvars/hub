import { useState } from "react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import * as api from "../api/commands";
import type { ReaderSettings, Theme } from "../types";

type Tab = "apariencia" | "lectura" | "datos" | "avanzado";
type Notice = { kind: "ok" | "err"; text: string } | null;

type Accent = "teal" | "plum" | "olive" | "copper";
type Density = "comodo" | "compacto";

interface SettingsProps {
  theme: Theme;
  accent: Accent;
  density: Density;
  readerSettings: ReaderSettings;
  intervalMinutes: number;
  similarityThreshold: number;
  embeddingStatus: [number, number] | null;
  saving: boolean;
  /** Aplica los cambios de apariencia en vivo (preview sin guardar). */
  onApplyAppearance: (theme: Theme, rs: ReaderSettings) => void;
  onAccentChange: (accent: Accent) => void;
  onDensityChange: (density: Density) => void;
  onSave: (theme: Theme, rs: ReaderSettings, interval: number, threshold: number) => void;
  onNotice: (n: Notice) => void;
  onReloadSources: () => void;
  onClose: () => void;
}

const TABS: { id: Tab; label: string }[] = [
  { id: "apariencia", label: "Apariencia" },
  { id: "lectura", label: "Lectura" },
  { id: "datos", label: "Datos" },
  { id: "avanzado", label: "Avanzado" },
];

const ACCENTS: { value: Accent; label: string }[] = [
  { value: "teal", label: "Petróleo" },
  { value: "plum", label: "Ciruela" },
  { value: "olive", label: "Oliva" },
  { value: "copper", label: "Cobre" },
];

const DENSITIES: { value: Density; label: string }[] = [
  { value: "comodo", label: "Cómodo" },
  { value: "compacto", label: "Compacto" },
];

const FONT_FAMILY_OPTS: { value: ReaderSettings["font_family"]; label: string }[] = [
  { value: "serif", label: "Serif" },
  { value: "sans", label: "Sans" },
  { value: "mono", label: "Mono" },
];

const LINE_HEIGHT_OPTS: { value: ReaderSettings["line_height"]; label: string }[] = [
  { value: "compact", label: "Compacto" },
  { value: "normal", label: "Normal" },
  { value: "relaxed", label: "Relajado" },
];

const WIDTH_OPTS: { value: ReaderSettings["width"]; label: string }[] = [
  { value: "narrow", label: "Estrecho" },
  { value: "medium", label: "Medio" },
  { value: "wide", label: "Ancho" },
];

export function Settings({
  theme: initialTheme,
  accent,
  density,
  readerSettings: initialRs,
  intervalMinutes,
  similarityThreshold,
  embeddingStatus,
  saving,
  onApplyAppearance,
  onAccentChange,
  onDensityChange,
  onSave,
  onNotice,
  onReloadSources,
  onClose,
}: SettingsProps) {
  const [tab, setTab] = useState<Tab>("apariencia");
  const [theme, setTheme] = useState<Theme>(initialTheme);
  const [rs, setRs] = useState<ReaderSettings>(initialRs);
  const [interval, setInterval] = useState(intervalMinutes);
  const [threshold, setThreshold] = useState(similarityThreshold);
  const [opmlBusy, setOpmlBusy] = useState(false);
  const [opmlMsg, setOpmlMsg] = useState<{ kind: "ok" | "err"; text: string } | null>(null);

  function updateTheme(next: Theme) {
    setTheme(next);
    onApplyAppearance(next, rs);
  }

  function updateRs(next: ReaderSettings) {
    setRs(next);
    onApplyAppearance(theme, next);
  }

  function set<T extends keyof ReaderSettings>(key: T, value: ReaderSettings[T]) {
    updateRs({ ...rs, [key]: value });
  }

  async function handleExport() {
    const path = await saveDialog({
      defaultPath: "fuentes.opml",
      filters: [{ name: "OPML", extensions: ["opml"] }],
    });
    if (!path) return;
    setOpmlBusy(true);
    setOpmlMsg(null);
    try {
      const n = await api.exportOpml(path);
      setOpmlMsg({ kind: "ok", text: `Exportadas ${n} fuentes a ${path}` });
      onNotice({ kind: "ok", text: `Exportadas ${n} fuentes` });
    } catch (e) {
      setOpmlMsg({ kind: "err", text: String(e) });
      onNotice({ kind: "err", text: String(e) });
    } finally {
      setOpmlBusy(false);
    }
  }

  async function handleImport() {
    const path = await openDialog({
      multiple: false,
      filters: [{ name: "OPML", extensions: ["opml", "xml"] }],
    });
    if (!path) return;
    setOpmlBusy(true);
    setOpmlMsg(null);
    try {
      const n = await api.importOpml(path);
      setOpmlMsg({ kind: "ok", text: `Importadas ${n} fuentes` });
      onNotice({ kind: "ok", text: `Importadas ${n} fuentes` });
      onReloadSources();
    } catch (e) {
      setOpmlMsg({ kind: "err", text: String(e) });
      onNotice({ kind: "err", text: String(e) });
    } finally {
      setOpmlBusy(false);
    }
  }

  return (
    <div className="settings-overlay" onClick={onClose}>
      <div
        className="settings-panel"
        role="dialog"
        aria-modal="true"
        aria-label="Configuración"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="settings-tabs" role="tablist" aria-label="Secciones de configuración">
          {TABS.map((t) => (
            <button
              key={t.id}
              role="tab"
              aria-selected={tab === t.id}
              className={`settings-tab${tab === t.id ? " is-selected" : ""}`}
              onClick={() => setTab(t.id)}
            >
              {t.label}
            </button>
          ))}
        </div>

        <div className="settings-body">
          {tab === "apariencia" && (
            <div className="settings-section">
              <label className="settings-field">
                <span>Tema</span>
                <select
                  value={theme}
                  onChange={(e) => updateTheme(e.target.value as Theme)}
                >
                  <option value="system">Seguir al sistema</option>
                  <option value="light">Claro</option>
                  <option value="dark">Oscuro</option>
                  <option value="sepia">Sepia</option>
                </select>
              </label>

              <div className="settings-field">
                <span>Color de acento</span>
                <div className="seg">
                  {ACCENTS.map((a) => (
                    <button
                      key={a.value}
                      className={`seg-opt${accent === a.value ? " is-selected" : ""}`}
                      onClick={() => onAccentChange(a.value)}
                    >
                      {a.label}
                    </button>
                  ))}
                </div>
              </div>

              <div className="settings-field">
                <span>Densidad</span>
                <div className="seg">
                  {DENSITIES.map((d) => (
                    <button
                      key={d.value}
                      className={`seg-opt${density === d.value ? " is-selected" : ""}`}
                      onClick={() => onDensityChange(d.value)}
                    >
                      {d.label}
                    </button>
                  ))}
                </div>
              </div>

              <label className="settings-field">
                <span>Tamaño de fuente del lector</span>
                <div className="range-field">
                  <input
                    type="range"
                    min={14}
                    max={28}
                    step={1}
                    value={rs.font_size}
                    onChange={(e) => set("font_size", Number(e.target.value))}
                  />
                  <span className="range-value">{rs.font_size}px</span>
                </div>
              </label>

              <div className="settings-field">
                <span>Familia tipográfica</span>
                <div className="seg">
                  {FONT_FAMILY_OPTS.map((o) => (
                    <button
                      key={o.value}
                      className={`seg-opt${rs.font_family === o.value ? " is-selected" : ""}`}
                      onClick={() => set("font_family", o.value)}
                    >
                      {o.label}
                    </button>
                  ))}
                </div>
              </div>

              <div className="settings-field">
                <span>Interlineado</span>
                <div className="seg">
                  {LINE_HEIGHT_OPTS.map((o) => (
                    <button
                      key={o.value}
                      className={`seg-opt${rs.line_height === o.value ? " is-selected" : ""}`}
                      onClick={() => set("line_height", o.value)}
                    >
                      {o.label}
                    </button>
                  ))}
                </div>
              </div>

              <div className="settings-field">
                <span>Ancho de columna</span>
                <div className="seg">
                  {WIDTH_OPTS.map((o) => (
                    <button
                      key={o.value}
                      className={`seg-opt${rs.width === o.value ? " is-selected" : ""}`}
                      onClick={() => set("width", o.value)}
                    >
                      {o.label}
                    </button>
                  ))}
                </div>
              </div>
              <p className="field-hint">Los cambios de apariencia se aplican al instante; pulsa "Guardar" para conservarlos.</p>
            </div>
          )}

          {tab === "lectura" && (
            <div className="settings-section">
              <label className="settings-field settings-toggle">
                <span>
                  Mostrar snippets en la lista
                  <small>Fragmento del texto junto al título en los resultados de búsqueda.</small>
                </span>
                <input
                  type="checkbox"
                  checked={rs.show_snippets}
                  onChange={(e) => set("show_snippets", e.target.checked)}
                />
              </label>
            </div>
          )}

          {tab === "datos" && (
            <div className="settings-section">
              <p className="field-hint">
                Exporta o importa tus fuentes en formato OPML, compatible con la mayoría de
                lectores de feeds.
              </p>
              <div className="settings-actions">
                <button className="settings-btn" onClick={() => void handleExport()} disabled={opmlBusy}>
                  {opmlBusy ? "Exportando" : "Exportar fuentes (OPML)"}
                </button>
                <button className="settings-btn" onClick={() => void handleImport()} disabled={opmlBusy}>
                  {opmlBusy ? "Importando" : "Importar fuentes (OPML)"}
                </button>
              </div>
              {opmlMsg && (
                <p className={`field-hint ${opmlMsg.kind === "err" ? "settings-error" : ""}`}>
                  {opmlMsg.text}
                </p>
              )}
              <p className="field-hint">
                Al importar, las fuentes con el mismo URL de feed se actualizan; el resto se añaden.
              </p>
            </div>
          )}

          {tab === "avanzado" && (
            <div className="settings-section">
              <label className="settings-field">
                <span>Refresco automático (minutos)</span>
                <input
                  type="number"
                  min={1}
                  value={interval}
                  onChange={(e) => setInterval(Number(e.target.value))}
                />
              </label>

              <label className="settings-field">
                <span>Similitud mínima para búsqueda semántica</span>
                <div className="range-field">
                  <input
                    type="range"
                    min={0}
                    max={1}
                    step={0.05}
                    value={threshold}
                    onChange={(e) => setThreshold(Number(e.target.value))}
                  />
                  <span className="range-value">{threshold.toFixed(2)}</span>
                </div>
                <span className="field-hint">
                  Más alto = resultados más estrictos (solo artículos muy parecidos)
                </span>
              </label>

              {embeddingStatus && (
                <div className="settings-field">
                  <span>Embeddings</span>
                  <span className="field-hint">
                    {embeddingStatus[0]} / {embeddingStatus[1]} artículos con embedding semántico
                  </span>
                  <span className="field-hint">
                    Modelo: all-MiniLM-L6-v2 (solo inglés). Los embeddings pendientes se generan en
                    segundo plano al abrir la app.
                  </span>
                </div>
              )}
            </div>
          )}
        </div>

        <div className="settings-actions modal-actions">
          <button className="modal-cancel" onClick={onClose}>
            Cancelar
          </button>
          <button
            className="modal-save"
            onClick={() => onSave(theme, rs, interval, threshold)}
            disabled={saving}
          >
            {saving ? "Guardando" : "Guardar"}
          </button>
        </div>
      </div>
    </div>
  );
}
