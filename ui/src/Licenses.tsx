import { useEffect, useMemo, useRef, useState } from "react";
import { HelpLink } from "./helpLinks";
import { Sheet } from "./sheets";

interface LicenseDocument {
  title: string;
  path: string;
}
interface LicenseCatalog {
  version: string;
  sourceUrl: string;
  documents: LicenseDocument[];
}
const documentKey = (doc: LicenseDocument) => `${doc.path}|${doc.title}`;

export function Licenses({ onClose }: { onClose: () => void }) {
  const [catalog, setCatalog] = useState<LicenseCatalog | null>(null);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState("");
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [catalogAttempt, setCatalogAttempt] = useState(0);
  const [documentAttempt, setDocumentAttempt] = useState(0);
  const reader = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    const request = new AbortController();
    setCatalogError(null);
    void fetch("/licenses/catalog.json", {
      signal: request.signal,
      cache: catalogAttempt ? "reload" : "default",
    })
      .then(async (response) => {
        if (!response.ok) throw Error("catalog");
        const value = (await response.json()) as LicenseCatalog;
        if (
          !Array.isArray(value.documents) ||
          !value.documents.length ||
          !value.documents.every(
            (doc) =>
              typeof doc.title === "string" &&
              typeof doc.path === "string" &&
              /^\/licenses\/[a-zA-Z0-9_./-]+$/.test(doc.path) &&
              !doc.path.includes(".."),
          )
        )
          throw Error("catalog");
        if (!request.signal.aborted) {
          setCatalog(value);
          setSelected(
            value.documents[0] ? documentKey(value.documents[0]) : "",
          );
        }
      })
      .catch(() => {
        if (!request.signal.aborted)
          setCatalogError("Could not load the bundled licenses. Try again.");
      });
    return () => request.abort();
  }, [catalogAttempt]);
  const matches = useMemo(
    () =>
      catalog?.documents.filter((doc) =>
        doc.title.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
      ) ?? [],
    [catalog, query],
  );
  const current = catalog?.documents.find(
    (doc) => documentKey(doc) === selected,
  );
  useEffect(() => {
    const request = new AbortController();
    setContent(null);
    if (!current) return;
    setError(null);
    void fetch(current.path, {
      signal: request.signal,
      cache: documentAttempt ? "reload" : "default",
    })
      .then(async (response) => {
        if (!response.ok) throw Error("document");
        const text = await response.text();
        if (!request.signal.aborted) setContent(text);
      })
      .catch(() => {
        if (!request.signal.aborted)
          setError("Could not load this license. Try again.");
      });
    return () => request.abort();
  }, [current, documentAttempt]);
  useEffect(() => {
    if (content !== null && reader.current) reader.current.scrollTop = 0;
  }, [content]);
  return (
    <Sheet
      title="Licenses"
      onClose={onClose}
      initialFocus='input[type="search"]'
    >
      <div className="license-browser">
        <p className="license-intro">
          Echocord and third-party licenses are included for offline reading.{" "}
          <HelpLink to="sources">Get corresponding sources</HelpLink>
        </p>
        <label className="license-search">
          <span className="sr-only">Find a license</span>
          <input
            type="search"
            placeholder="Find a package or license"
            value={query}
            onChange={(event) => setQuery(event.currentTarget.value)}
          />
        </label>
        <label className="license-choice">
          <span className="sr-only">License document</span>
          <select
            value={current ? documentKey(current) : ""}
            onChange={(event) => setSelected(event.currentTarget.value)}
            disabled={!matches.length}
          >
            {current && !matches.includes(current) && (
              <option value={documentKey(current)}>
                {current.title} (open)
              </option>
            )}
            {matches.map((doc) => (
              <option key={documentKey(doc)} value={documentKey(doc)}>
                {doc.title}
              </option>
            ))}
          </select>
        </label>
        {catalog && (
          <span className="license-count">
            {query
              ? `${matches.length.toLocaleString()} matching ${matches.length === 1 ? "document" : "documents"}`
              : `${catalog.documents.length.toLocaleString()} documents`}
          </span>
        )}
        {error || catalogError ? (
          <div className="license-error" role="alert">
            <p>{catalogError ?? error}</p>
            <button
              className="btn"
              type="button"
              onClick={() => {
                if (catalogError) setCatalogAttempt(catalogAttempt + 1);
                else setDocumentAttempt(documentAttempt + 1);
              }}
            >
              Try again
            </button>
          </div>
        ) : !catalog || (current && content === null) ? (
          <p role="status">Loading licenses…</p>
        ) : current ? (
          <textarea
            ref={reader}
            className="license-text"
            readOnly
            aria-label={current.title}
            value={content ?? ""}
            spellCheck={false}
          />
        ) : (
          <p role="status">No matching licenses. Try a different name.</p>
        )}
      </div>
    </Sheet>
  );
}
