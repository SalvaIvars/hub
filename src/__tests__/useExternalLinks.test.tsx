import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { useExternalLinks } from "../hooks/useExternalLinks";
import { openExternal } from "../api/commands";

vi.mock("../api/commands", () => ({
  openExternal: vi.fn(() => Promise.resolve()),
}));

function Harness({ url }: { url: string }) {
  useExternalLinks(url);
  return (
    <div>
      <p className="reader-content">
        <a id="ext" href="https://example.com/a" target="_blank">
          enlace
        </a>
        <a id="rel" href="/rel" target="_blank">
          relativo
        </a>
        <a id="mail" href="mailto:a@b.c" target="_blank">
          mail
        </a>
        <a id="js" href="javascript:alert(1)" target="_blank">
          js
        </a>
        <a id="hash" href="#frag" target="_blank">
          ancla
        </a>
      </p>
      <a id="outside" href="https://fuera.com">
        fuera del lector
      </a>
    </div>
  );
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("useExternalLinks (clics en enlaces del lector)", () => {
  it("abre enlaces absolutos http/https en el navegador", () => {
    render(<Harness url="https://site.com/post/1" />);
    fireEvent.click(document.getElementById("ext")!);
    expect(openExternal).toHaveBeenCalledTimes(1);
    expect(openExternal).toHaveBeenCalledWith("https://example.com/a");
  });

  it("resuelve enlaces relativos contra la URL del artículo", () => {
    render(<Harness url="https://site.com/post/1" />);
    fireEvent.click(document.getElementById("rel")!);
    expect(openExternal).toHaveBeenCalledTimes(1);
    expect(openExternal).toHaveBeenCalledWith("https://site.com/rel");
  });

  it("abre mailto: en la app de correo", () => {
    render(<Harness url="https://site.com/post/1" />);
    fireEvent.click(document.getElementById("mail")!);
    expect(openExternal).toHaveBeenCalledWith("mailto:a@b.c");
  });

  it("bloquea javascript:", () => {
    render(<Harness url="https://site.com/post/1" />);
    fireEvent.click(document.getElementById("js")!);
    expect(openExternal).not.toHaveBeenCalled();
  });

  it("ignora anclas internas #fragment", () => {
    render(<Harness url="https://site.com/post/1" />);
    fireEvent.click(document.getElementById("hash")!);
    expect(openExternal).not.toHaveBeenCalled();
  });

  it("no intercepta enlaces fuera de .reader-content", () => {
    render(<Harness url="https://site.com/post/1" />);
    fireEvent.click(document.getElementById("outside")!);
    expect(openExternal).not.toHaveBeenCalled();
  });

  it("previene la navegación por defecto del webview", () => {
    render(<Harness url="https://site.com/post/1" />);
    let prevented = false;
    // Listener en fase de captura sobre document: se ejecuta tras el del hook
    // (mismo nodo y fase, orden de registro) pero antes de que el evento siga
    // propagándose, y no se ve afectado por stopPropagation().
    document.addEventListener(
      "click",
      (e) => {
        prevented = e.defaultPrevented;
      },
      true,
    );
    fireEvent.click(document.getElementById("ext")!);
    expect(prevented).toBe(true);
  });
});
