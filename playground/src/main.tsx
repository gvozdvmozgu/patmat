import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  CircleHelp,
  Code2,
  ListChecks,
  TriangleAlert,
} from "lucide-react";
import React, { useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import init, { analyze_dsl } from "patmat-playground-wasm";
import "./styles.css";

type Span = {
  start: number;
  end: number;
};

type Arm = {
  index: number;
  source: string;
  span: Span;
  space: string;
  reachable: boolean;
};

type Warning =
  | {
      kind: "Unreachable";
      armIndex: number;
      coveringArmIndices: number[];
    }
  | {
      kind: "OnlyNull";
      armIndex: number;
      coveringArmIndices: number[];
    };

type TypeDecl = {
  name: string;
  params: string[];
  constructors: string[];
};

type SuccessResult = {
  ok: true;
  scrutinee: string;
  isExhaustive: boolean;
  uncovered: string[];
  warnings: Warning[];
  arms: Arm[];
  types: TypeDecl[];
};

type ErrorResult = {
  ok: false;
  error: string;
  span: Span | null;
};

type AnalysisResult = SuccessResult | ErrorResult;
type GuidanceTab = "syntax" | "types" | "patterns" | "examples";

const examples = [
  {
    name: "Exhaustive Bool",
    description: "A complete two-constructor match.",
    source: `type Bool =
  | true
  | false

match Bool:
  true
  false
`,
  },
  {
    name: "Missing false",
    description: "Shows one uncovered constructor.",
    source: `type Bool =
  | true
  | false

match Bool:
  true
`,
  },
  {
    name: "Option Bool",
    description: "Covers nested Bool values with separate arms.",
    source: `type Bool =
  | true
  | false

type Option<T> =
  | Some(T)
  | None

match Option<Bool>:
  Some(true)
  Some(false)
  None
`,
  },
  {
    name: "Or-pattern Option",
    description: "Uses `true | false` inside a constructor pattern.",
    source: `type Bool =
  | true
  | false

type Option<T> =
  | Some(T)
  | None

match Option<Bool>:
  Some(true | false)
  None
`,
  },
  {
    name: "Unreachable wildcard",
    description: "A wildcard shadows a later arm.",
    source: `type Bool =
  | true
  | false

match Bool:
  _
  true
`,
  },
  {
    name: "Nested Result / Option",
    description: "Combines generic ADTs and nested patterns.",
    source: `type Bool =
  | true
  | false

type Option<T> =
  | Some(T)
  | None

type Result<T, E> =
  | Ok(T)
  | Err(E)

match Result<Bool, Option<Bool>>:
  Ok(true)
  Ok(false)
  Err(Some(true))
  Err(Some(false))
  Err(None)
`,
  },
];

function App() {
  const [source, setSource] = useState(examples[2].source);
  const [ready, setReady] = useState(false);
  const [result, setResult] = useState<AnalysisResult | null>(null);
  const [selectedLine, setSelectedLine] = useState<number | null>(null);
  const [previewLine, setPreviewLine] = useState<number | null>(null);
  const [previewArmIndex, setPreviewArmIndex] = useState<number | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [scrollLeft, setScrollLeft] = useState(0);
  const [guidanceTab, setGuidanceTab] = useState<GuidanceTab>("syntax");
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    init()
      .then(() => setReady(true))
      .catch((error) =>
        setResult({
          ok: false,
          error: error instanceof Error ? error.message : String(error),
          span: null,
        }),
      );
  }, []);

  useEffect(() => {
    if (!ready) {
      return;
    }

    if (source.trim().length === 0) {
      setResult(null);
      return;
    }

    const handle = window.setTimeout(() => {
      try {
        setResult(analyze_dsl(source) as AnalysisResult);
      } catch (error) {
        setResult({
          ok: false,
          error: error instanceof Error ? error.message : String(error),
          span: null,
        });
      }
    }, 180);

    return () => window.clearTimeout(handle);
  }, [ready, source]);

  const lines = useMemo(() => source.split("\n"), [source]);
  const currentExampleName = useMemo(
    () => examples.find((example) => example.source === source)?.name ?? "Custom program",
    [source],
  );
  const armLines = useMemo(() => {
    if (!result?.ok) {
      return new Map<number, number>();
    }
    return new Map(result.arms.map((arm) => [arm.index, positionForOffset(source, arm.span.start).line]));
  }, [result, source]);
  const armLineLookup = useMemo(() => {
    const lookup = new Map<number, number>();
    armLines.forEach((line, armIndex) => lookup.set(line, armIndex));
    return lookup;
  }, [armLines]);

  const errorLineColumn = useMemo(() => {
    if (!result || result.ok || !result.span) {
      return null;
    }
    return positionForOffset(source, result.span.start);
  }, [result, source]);

  function focusSpan(span: Span) {
    const textarea = textareaRef.current;
    if (!textarea) {
      return;
    }

    const position = positionForOffset(source, span.start);
    setSelectedLine(position.line);
    setPreviewLine(null);
    setPreviewArmIndex(null);
    textarea.focus();
    textarea.setSelectionRange(span.start, span.end);
  }

  function focusLine(line: number) {
    const textarea = textareaRef.current;
    if (!textarea) {
      return;
    }

    const { start, end } = spanForLine(source, line);
    setSelectedLine(line);
    setPreviewLine(null);
    setPreviewArmIndex(null);
    textarea.focus();
    textarea.setSelectionRange(start, end);
  }

  function previewEditorLineFromPointer(clientY: number) {
    const textarea = textareaRef.current;
    if (!textarea) {
      return;
    }

    const line = lineForEditorPointer(textarea, clientY);
    const armIndex = armLineLookup.get(line);
    if (armIndex === undefined) {
      setPreviewLine(null);
      setPreviewArmIndex(null);
      return;
    }

    setPreviewLine(line);
    setPreviewArmIndex(armIndex);
  }

  function clearPreview() {
    setPreviewLine(null);
    setPreviewArmIndex(null);
  }

  function insertMissingPattern(pattern: string) {
    const insertion = `${source.endsWith("\n") ? "" : "\n"}  ${pattern}\n`;
    const nextSource = `${source}${insertion}`;
    setSource(nextSource);
    window.setTimeout(() => focusLine(nextSource.split("\n").length - 1), 0);
  }

  return (
    <main className="app-shell">
      <header className="masthead">
        <div>
          <h1>patmat playground</h1>
          <p>Write a small ADT program and see which match arms are missing or unreachable.</p>
        </div>
      </header>

      <section className="workbench">
        <section className="panel program-panel" aria-label="Program">
          <PanelHeader
            icon={<Code2 aria-hidden="true" />}
            title="Program"
            detail="write DSL"
            control={
              <label className="example-select">
                <span>Load example:</span>
                <select
                  value={currentExampleName}
                  onChange={(event) => {
                    const example = examples.find((item) => item.name === event.target.value);
                    if (example) {
                      setSource(example.source);
                      setSelectedLine(null);
                      clearPreview();
                    }
                  }}
                >
                  <option value="Custom program" disabled>
                    Custom program
                  </option>
                  {examples.map((example) => (
                    <option value={example.name} key={example.name}>
                      {example.name}
                    </option>
                  ))}
                </select>
                <ChevronDown aria-hidden="true" />
              </label>
            }
          />

          <div className="editor-shell">
            <div className="line-gutter" style={{ transform: `translateY(${-scrollTop}px)` }}>
              {lines.map((_, index) => (
                <button
                  type="button"
                  key={`${index + 1}-${lines.length}`}
                  className={
                    selectedLine === index + 1 || previewLine === index + 1 ? "active-line" : ""
                  }
                  onClick={() => focusLine(index + 1)}
                >
                  {index + 1}
                </button>
              ))}
            </div>
            <pre
              className="syntax-layer"
              style={{ transform: `translate(${-scrollLeft}px, ${-scrollTop}px)` }}
            >
              {lines.map((line, index) => (
                <span
                  className={
                    selectedLine === index + 1 || previewLine === index + 1
                      ? "syntax-line active"
                      : "syntax-line"
                  }
                  key={`${index}-${line}`}
                >
                  <HighlightedLine text={line} />
                </span>
              ))}
            </pre>
            <textarea
              ref={textareaRef}
              spellCheck={false}
              wrap="off"
              value={source}
              onChange={(event) => {
                setSource(event.target.value);
                clearPreview();
              }}
              onMouseMove={(event) => previewEditorLineFromPointer(event.clientY)}
              onMouseLeave={clearPreview}
              onScroll={(event) => {
                setScrollTop(event.currentTarget.scrollTop);
                setScrollLeft(event.currentTarget.scrollLeft);
              }}
              aria-label="patmat DSL source"
            />
          </div>

          <EditorDiagnostic
            result={result}
            ready={ready}
            errorLineColumn={errorLineColumn}
            onFocusLine={focusLine}
          />
        </section>

        <section className="panel analysis-panel" aria-label="Analysis">
          <PanelHeader icon={<ListChecks aria-hidden="true" />} title="Analysis" detail="compiler feedback" />
          {!ready && <EmptyDiagnostic title="Preparing analyzer..." body="The WebAssembly checker is loading." />}
          {ready && source.trim().length === 0 && (
            <EmptyDiagnostic
              title="Start by declaring a type and a match block."
              body="Analysis appears here as soon as the program is valid enough to inspect."
            />
          )}
          {ready && source.trim().length > 0 && result?.ok && (
            <AnalysisPanel
              result={result}
              armLines={armLines}
              onInsertMissing={insertMissingPattern}
              onFocusArm={(arm) => focusSpan(arm.span)}
              onFocusLine={focusLine}
              onPreviewLine={setPreviewLine}
              previewArmIndex={previewArmIndex}
              onPreviewArm={setPreviewArmIndex}
            />
          )}
          {ready && source.trim().length > 0 && result && !result.ok && (
            <ErrorPanel result={result} lineColumn={errorLineColumn} onFocusLine={focusLine} />
          )}
        </section>
      </section>

      <Guidance activeTab={guidanceTab} onTabChange={setGuidanceTab} />
    </main>
  );
}

function PanelHeader({
  icon,
  title,
  detail,
  control,
}: {
  icon: React.ReactNode;
  title: string;
  detail: string;
  control?: React.ReactNode;
}) {
  return (
    <header className="panel-header">
      <div>
        {icon}
        <h2>{title}</h2>
        <span>{detail}</span>
      </div>
      {control}
    </header>
  );
}

function AnalysisPanel({
  result,
  armLines,
  onInsertMissing,
  onFocusArm,
  onFocusLine,
  onPreviewLine,
  previewArmIndex,
  onPreviewArm,
}: {
  result: SuccessResult;
  armLines: Map<number, number>;
  onInsertMissing: (pattern: string) => void;
  onFocusArm: (arm: Arm) => void;
  onFocusLine: (line: number) => void;
  onPreviewLine: (line: number | null) => void;
  previewArmIndex: number | null;
  onPreviewArm: (armIndex: number | null) => void;
}) {
  const unreachableCount = result.warnings.filter((warning) => warning.kind === "Unreachable").length;
  const missingCount = result.uncovered.length;
  const summary = result.isExhaustive
    ? `${result.scrutinee} is fully covered.`
    : `The match does not cover: ${result.uncovered.join(", ")}.`;
  const label = result.isExhaustive ? "Exhaustive" : "Missing cases";
  const tone = !result.isExhaustive ? "warning" : unreachableCount > 0 ? "notice" : "success";

  return (
    <div className="analysis-stack">
      <DiagnosticSummary label={label} detail={summary} tone={tone} />
      {(missingCount > 0 || unreachableCount > 0) && (
        <p className="count-line">
          {missingCount > 0 && `${plural(missingCount, "case is", "cases are")} missing.`}
          {unreachableCount > 0 &&
            ` ${plural(unreachableCount, "arm is", "arms are")} unreachable.`}
        </p>
      )}

      {result.uncovered.length > 0 && (
        <section className="diagnostic-section">
          <h3>Missing patterns</h3>
          <div className="diagnostic-list">
            {result.uncovered.map((space) => (
              <div
                key={space}
                className="diagnostic-row missing-row"
              >
                <span className="badge missing">missing</span>
                <code>{space}</code>
                <button type="button" className="quick-action" onClick={() => onInsertMissing(space)}>
                  Insert arm
                </button>
              </div>
            ))}
          </div>
        </section>
      )}

      {result.warnings.length > 0 && (
        <section className="diagnostic-section">
          <h3>Reachability warnings</h3>
          <div className="diagnostic-list">
            {result.warnings.map((warning) => (
              <WarningRow
                key={`${warning.kind}-${warning.armIndex}`}
                warning={warning}
                armLines={armLines}
                onFocusLine={onFocusLine}
                onPreviewLine={onPreviewLine}
              />
            ))}
          </div>
        </section>
      )}

      <section className="diagnostic-section">
        <h3>Arm reachability</h3>
        <div className="arm-table" role="table" aria-label="Arm reachability">
          <div className="arm-row header" role="row">
            <span>Line</span>
            <span>Pattern</span>
            <span>Space</span>
            <span>Result</span>
          </div>
          {result.arms.length === 0 ? (
            <p className="quiet table-empty">No match arms yet.</p>
          ) : (
            result.arms.map((arm) => (
              <div
                className={previewArmIndex === arm.index ? "arm-row active" : "arm-row"}
                role="row"
                key={`${arm.index}-${arm.source}`}
                onMouseEnter={() => {
                  const line = armLines.get(arm.index);
                  if (line) {
                    onPreviewLine(line);
                    onPreviewArm(arm.index);
                  }
                }}
                onMouseLeave={() => {
                  onPreviewLine(null);
                  onPreviewArm(null);
                }}
                onFocus={() => {
                  const line = armLines.get(arm.index);
                  if (line) {
                    onPreviewLine(line);
                    onPreviewArm(arm.index);
                  }
                }}
                onBlur={() => {
                  onPreviewLine(null);
                  onPreviewArm(null);
                }}
              >
                <button
                  type="button"
                  className="line-link"
                  onClick={() => onFocusArm(arm)}
                  aria-label={`Focus line ${armLines.get(arm.index) ?? "unknown"}`}
                >
                  {armLines.get(arm.index) ?? "-"}
                </button>
                <code>{arm.source}</code>
                <code>{arm.space}</code>
                <span className={`badge ${arm.reachable ? "reachable" : "unreachable"}`}>
                  {arm.reachable ? "reachable" : "unreachable"}
                </span>
              </div>
            ))
          )}
        </div>
      </section>

      <details className="details-block">
        <summary>Parsed types and spaces</summary>
        <div className="type-list">
          {result.types.map((typeDecl) => (
            <div key={typeDecl.name} className="type-item">
              <code>
                {typeDecl.name}
                {typeDecl.params.length > 0 ? `<${typeDecl.params.join(", ")}>` : ""}
              </code>
              <span>{typeDecl.constructors.join(" | ")}</span>
            </div>
          ))}
        </div>
      </details>
    </div>
  );
}

function WarningRow({
  warning,
  armLines,
  onFocusLine,
  onPreviewLine,
}: {
  warning: Warning;
  armLines: Map<number, number>;
  onFocusLine: (line: number) => void;
  onPreviewLine: (line: number | null) => void;
}) {
  const line = armLines.get(warning.armIndex);
  const coveringLines = warning.coveringArmIndices
    .map((index) => armLines.get(index))
    .filter((value): value is number => value !== undefined);
  const detail =
    warning.kind === "Unreachable"
      ? `is already covered by ${formatLines(coveringLines)}.`
      : `is only reachable for null; non-null values are covered by ${formatLines(coveringLines)}.`;

  return (
    <div
      className="diagnostic-row"
      onMouseEnter={() => onPreviewLine(line ?? null)}
      onMouseLeave={() => onPreviewLine(null)}
    >
      <span className="badge unreachable">{warning.kind === "Unreachable" ? "unreachable" : "only null"}</span>
      <span>
        {line ? (
          <button type="button" className="inline-line-link" onClick={() => onFocusLine(line)}>
            Line {line}
          </button>
        ) : (
          "Line ?"
        )}
        {` ${detail}`}
      </span>
    </div>
  );
}

function ErrorPanel({
  result,
  lineColumn,
  onFocusLine,
}: {
  result: ErrorResult;
  lineColumn: { line: number; column: number } | null;
  onFocusLine: (line: number) => void;
}) {
  return (
    <div className="analysis-stack">
      <DiagnosticSummary
        label="Cannot analyze program"
        detail={
          lineColumn
            ? `${result.error} on line ${lineColumn.line}.`
            : result.error
        }
        tone="danger"
      />
      <p className="count-line">Fix the parse error before analysis can run.</p>
      {lineColumn && (
        <button type="button" className="diagnostic-row" onClick={() => onFocusLine(lineColumn.line)}>
          <span className="badge error">error</span>
          <span>
            Line {lineColumn.line}, column {lineColumn.column}
          </span>
        </button>
      )}
    </div>
  );
}

function EditorDiagnostic({
  result,
  ready,
  errorLineColumn,
  onFocusLine,
}: {
  result: AnalysisResult | null;
  ready: boolean;
  errorLineColumn: { line: number; column: number } | null;
  onFocusLine: (line: number) => void;
}) {
  if (!ready) {
    return <p className="editor-hint">Preparing analyzer...</p>;
  }
  if (!result) {
    return <p className="editor-hint">Start by declaring a type and a match block.</p>;
  }
  if (!result.ok && errorLineColumn) {
    return (
      <button type="button" className="editor-hint error-hint" onClick={() => onFocusLine(errorLineColumn.line)}>
        Line {errorLineColumn.line}: {result.error}
      </button>
    );
  }
  if (!result.ok) {
    return <p className="editor-hint error-hint">{result.error}</p>;
  }
  return <p className="editor-hint">Ready · Results update automatically.</p>;
}

function DiagnosticSummary({
  label,
  detail,
  tone,
}: {
  label: string;
  detail: string;
  tone: "success" | "warning" | "danger" | "notice";
}) {
  const Icon = tone === "success" ? CheckCircle2 : tone === "danger" ? AlertCircle : TriangleAlert;
  return (
    <div className={`summary ${tone}`}>
      <Icon aria-hidden="true" />
      <div>
        <p>
          <strong>{label}</strong> — <span>{detail}</span>
        </p>
      </div>
    </div>
  );
}

function EmptyDiagnostic({ title, body }: { title: string; body: string }) {
  return (
    <div className="empty-state">
      <CircleHelp aria-hidden="true" />
      <strong>{title}</strong>
      <span>{body}</span>
    </div>
  );
}

function Guidance({
  activeTab,
  onTabChange,
}: {
  activeTab: GuidanceTab;
  onTabChange: (tab: GuidanceTab) => void;
}) {
  return (
    <details className="guidance">
      <summary>
        <div>
          <ChevronDown className="guidance-chevron" aria-hidden="true" />
          <h2>Guidance</h2>
        </div>
        <span>Syntax help and examples</span>
      </summary>
      <div className="guidance-panel">
        <div className="tabs" role="tablist" aria-label="Guidance tabs">
          {(["syntax", "types", "patterns", "examples"] as GuidanceTab[]).map((tab) => (
            <button
              key={tab}
              type="button"
              className={activeTab === tab ? "active" : ""}
              onClick={() => onTabChange(tab)}
            >
              {tab}
            </button>
          ))}
        </div>
        <div className="guidance-body">
        {activeTab === "syntax" && (
          <pre>{`type TypeName =
  | Constructor
  | Constructor(Type, Type)

match Type:
  pattern
  pattern`}</pre>
        )}
        {activeTab === "types" && (
          <pre>{`Bool
Option<Bool>
Result<Bool, Option<Bool>>

Generic declarations:
type Result<T, E> =
  | Ok(T)
  | Err(E)`}</pre>
        )}
        {activeTab === "patterns" && (
          <pre>{`_
Constructor
Constructor(_)
Constructor(left, right)
left | right
Constructor(left | right)

Names like true, false, and None are nullary constructors.`}</pre>
        )}
        {activeTab === "examples" && (
          <div className="example-list">
            {examples.map((example) => (
              <div className="example-reference" key={example.name}>
                <strong>{example.name}</strong>
                <span>{example.description}</span>
              </div>
            ))}
            <p>Load these from the Program example picker.</p>
          </div>
        )}
        </div>
      </div>
    </details>
  );
}

function HighlightedLine({ text }: { text: string }) {
  const parts = text.split(/(\b(?:type|match)\b|[_|=():,<>]|\b[A-Z][A-Za-z0-9_]*\b|\b[a-z][A-Za-z0-9_]*\b)/g);
  return (
    <>
      {parts.map((part, index) => {
        if (part === "") {
          return null;
        }
        const className =
          part === "type" || part === "match"
            ? "tok-keyword"
            : /^[A-Z]/.test(part)
              ? "tok-type"
              : /^[a-z]/.test(part)
                ? "tok-constructor"
                : /[_|=():,<>]/.test(part)
                  ? "tok-punct"
                  : undefined;
        return (
          <span className={className} key={`${part}-${index}`}>
            {part}
          </span>
        );
      })}
    </>
  );
}

function positionForOffset(source: string, offset: number) {
  let line = 1;
  let column = 1;
  for (let index = 0; index < offset; index += 1) {
    if (source[index] === "\n") {
      line += 1;
      column = 1;
    } else {
      column += 1;
    }
  }
  return { line, column };
}

function spanForLine(source: string, line: number) {
  let currentLine = 1;
  let start = 0;
  for (let index = 0; index < source.length; index += 1) {
    if (currentLine === line) {
      start = index;
      break;
    }
    if (source[index] === "\n") {
      currentLine += 1;
      start = index + 1;
    }
  }

  let end = source.indexOf("\n", start);
  if (end === -1) {
    end = source.length;
  }
  return { start, end };
}

function lineForEditorPointer(textarea: HTMLTextAreaElement, clientY: number) {
  const editorPaddingTop = 16;
  const editorLineHeight = 24;
  const { top } = textarea.getBoundingClientRect();
  const y = clientY - top + textarea.scrollTop - editorPaddingTop;
  return Math.max(1, Math.floor(y / editorLineHeight) + 1);
}

function plural(count: number, singular: string, pluralText: string) {
  return `${count} ${count === 1 ? singular : pluralText}`;
}

function formatLines(lines: number[]) {
  if (lines.length === 0) {
    return "earlier arms";
  }
  if (lines.length === 1) {
    return `line ${lines[0]}`;
  }
  return `lines ${lines.join(", ")}`;
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
