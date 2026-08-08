import { useRef, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Mock } from "vitest";
import { useAutoExtractArticle } from "../hooks/useAutoExtractArticle";
import { extractArticle, generateEmbedding, getArticle } from "../api/commands";
import type { Article } from "../types";

vi.mock("../api/commands", () => ({
  extractArticle: vi.fn(),
  generateEmbedding: vi.fn(),
  getArticle: vi.fn(),
}));

const mockExtractArticle = extractArticle as Mock;
const mockGenerateEmbedding = generateEmbedding as Mock;
const mockGetArticle = getArticle as Mock;

function makeArticle(id: number, html: string, text = ""): Article {
  return {
    id,
    source_id: null,
    url: `https://site.com/posts/${id}`,
    title: `Post ${id}`,
    html,
    text,
    raw_html: "",
    byline: null,
    site_name: null,
    published_at: null,
    fetched_at: "2024-01-01T00:00:00Z",
    read: false,
    starred: false,
    has_embedding: false,
  };
}

/** Envuelve el hook para poder observarlo y dispararlo desde el DOM. */
function Harness({ article }: { article: Article }) {
  const currentIdRef = useRef<number | null>(null);
  const [current, setCurrent] = useState<Article | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const { extracting, extract } = useAutoExtractArticle({
    getCurrentId: () => currentIdRef.current,
    onResult: setCurrent,
    onRefreshList: async () => {},
    onNotice: (n) => setNotice(n?.text ?? null),
  });
  return (
    <div>
      <button
        onClick={() => {
          currentIdRef.current = article.id;
          void extract(article);
        }}
      >
        abrir
      </button>
      <button
        onClick={() => {
          currentIdRef.current = -1;
        }}
      >
        cambiar
      </button>
      <span data-testid="current">{current ? current.html : "none"}</span>
      <span data-testid="extracting">{String(extracting)}</span>
      <span data-testid="notice">{notice ?? "none"}</span>
    </div>
  );
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("useAutoExtractArticle (extracción automática al abrir)", () => {
  it("extrae y aplica el contenido de un artículo sin cuerpo", async () => {
    const resumen = makeArticle(1, "");
    const extraido = makeArticle(1, "<p>contenido completo</p>", "contenido completo");
    mockExtractArticle.mockResolvedValue(extraido);

    render(<Harness article={resumen} />);
    fireEvent.click(screen.getByText("abrir"));

    await waitFor(() => expect(screen.getByTestId("current").textContent).toBe("<p>contenido completo</p>"));
    expect(extractArticle).toHaveBeenCalledTimes(1);
    expect(extractArticle).toHaveBeenCalledWith("https://site.com/posts/1");
    expect(screen.getByTestId("extracting").textContent).toBe("false");
  });

  it("no extrae si el artículo ya tiene cuerpo", async () => {
    const conCuerpo = makeArticle(2, "<p>ya extraído</p>");
    render(<Harness article={conCuerpo} />);
    fireEvent.click(screen.getByText("abrir"));
    await waitFor(() => expect(screen.getByTestId("extracting").textContent).toBe("false"));
    expect(extractArticle).not.toHaveBeenCalled();
  });

  it("no lanza una segunda extracción mientras hay una en curso", async () => {
    let resolve: (a: Article) => void = () => {};
    mockExtractArticle.mockImplementation(
      () => new Promise<Article>((r) => (resolve = r)),
    );

    const resumen = makeArticle(3, "");
    render(<Harness article={resumen} />);
    fireEvent.click(screen.getByText("abrir"));
    fireEvent.click(screen.getByText("abrir"));

    await waitFor(() => expect(screen.getByTestId("extracting").textContent).toBe("true"));
    expect(extractArticle).toHaveBeenCalledTimes(1);

    resolve(makeArticle(3, "<p>listo</p>"));
    await waitFor(() => expect(screen.getByTestId("extracting").textContent).toBe("false"));
  });

  it("descarta el resultado si el artículo ya no es el activo", async () => {
    let resolve: (a: Article) => void = () => {};
    mockExtractArticle.mockImplementation(
      () => new Promise<Article>((r) => (resolve = r)),
    );

    const resumen = makeArticle(4, "");
    render(<Harness article={resumen} />);
    fireEvent.click(screen.getByText("abrir"));
    // El usuario navega a otro artículo antes de que termine la extracción.
    fireEvent.click(screen.getByText("cambiar"));
    resolve(makeArticle(4, "<p>resultado obsoleto</p>"));

    await waitFor(() => expect(screen.getByTestId("extracting").textContent).toBe("false"));
    expect(screen.getByTestId("current").textContent).toBe("none");
  });

  it("regenera el embedding tras extraer texto y aplica el artículo refrescado", async () => {
    const resumen = makeArticle(5, "");
    const textoLargo = "texto ".repeat(20);
    mockExtractArticle.mockResolvedValue(makeArticle(5, "<p>extraído</p>", textoLargo));
    mockGenerateEmbedding.mockResolvedValue(undefined);
    mockGetArticle.mockResolvedValue({
      ...makeArticle(5, "<p>extraído</p>", textoLargo),
      has_embedding: true,
    });

    render(<Harness article={resumen} />);
    fireEvent.click(screen.getByText("abrir"));

    await waitFor(() => expect(mockGenerateEmbedding).toHaveBeenCalledWith(5));
    expect(mockGetArticle).toHaveBeenCalledWith(5);
    await waitFor(() =>
      expect(screen.getByTestId("notice").textContent).toBe(
        "Contenido extraído y embedding generado",
      ),
    );
  });

  it("notifica el error de extracción si el artículo sigue activo", async () => {
    mockExtractArticle.mockRejectedValue(new Error("red caída"));
    const resumen = makeArticle(6, "");
    render(<Harness article={resumen} />);
    fireEvent.click(screen.getByText("abrir"));

    await waitFor(() => expect(screen.getByTestId("extracting").textContent).toBe("false"));
    expect(screen.getByTestId("notice").textContent).toContain("red caída");
  });
});
