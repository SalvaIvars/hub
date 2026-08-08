import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Settings } from "../components/Settings";
import type { ReaderSettings } from "../types";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: vi.fn(),
}));

vi.mock("../api/commands", () => ({
  exportOpml: vi.fn(() => Promise.resolve(0)),
  importOpml: vi.fn(() => Promise.resolve(0)),
}));

const DEFAULT_RS: ReaderSettings = {
  font_size: 19,
  font_family: "serif",
  line_height: "normal",
  width: "medium",
  show_snippets: true,
};

function renderSettings(opts: { onPurge?: () => void; onSave?: () => void } = {}) {
  return render(
    <Settings
      theme="system"
      accent="teal"
      density="comodo"
      readerSettings={DEFAULT_RS}
      intervalMinutes={30}
      similarityThreshold={0.7}
      purgeDays={7}
      embeddingStatus={null}
      saving={false}
      purging={false}
      onApplyAppearance={() => {}}
      onAccentChange={() => {}}
      onDensityChange={() => {}}
      onSave={opts.onSave ?? (() => {})}
      onPurge={opts.onPurge ?? (() => {})}
      onNotice={() => {}}
      onReloadSources={() => {}}
      onClose={() => {}}
    />,
  );
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("Settings (limpieza de contenido extraído)", () => {
  it("muestra el campo de purga automática y el botón de purga inmediata en Avanzado", () => {
    renderSettings();
    fireEvent.click(screen.getByText("Avanzado"));
    expect(screen.getByText("Vaciar contenido extraído automáticamente tras N días")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Vaciar contenido de artículos leídos" })).toBeTruthy();
  });

  it("el botón de purga inmediata dispara onPurge", () => {
    const onPurge = vi.fn();
    renderSettings({ onPurge });
    fireEvent.click(screen.getByText("Avanzado"));
    fireEvent.click(screen.getByRole("button", { name: "Vaciar contenido de artículos leídos" }));
    expect(onPurge).toHaveBeenCalledTimes(1);
  });

  it("envía los días de purga al guardar", () => {
    const onSave = vi.fn();
    renderSettings({ onSave });
    fireEvent.click(screen.getByText("Avanzado"));
    const input = screen.getByLabelText(
      /Vaciar contenido extraído automáticamente/,
    ) as HTMLInputElement;
    expect(input.value).toBe("7");
    fireEvent.change(input, { target: { value: "30" } });
    fireEvent.click(screen.getByRole("button", { name: "Guardar" }));
    const args = onSave.mock.calls[0];
    expect(args).toBeDefined();
    expect(args[4]).toBe(30);
  });
});
