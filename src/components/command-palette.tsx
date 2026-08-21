import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { commands, type GlobalSearchResultDto } from "@/generated/tauri-bindings";
import { unwrapResult } from "@/lib/tauri/errors";

const STATIC_ACTIONS = [
  { key: "overview", path: "/overview" },
  { key: "investments", path: "/investments" },
  { key: "activity", path: "/activity" },
  { key: "newPending", path: "/maintenance" },
  { key: "maintenance", path: "/maintenance" },
  { key: "analytics", path: "/analytics" },
  { key: "settings", path: "/settings/general" },
] as const;

type PaletteEntry =
  | { kind: "action"; key: string; path: string }
  | { kind: "result"; result: GlobalSearchResultDto };

export function CommandPalette() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const [composing, setComposing] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const restoreFocusRef = useRef<HTMLElement | null>(null);

  const search = useQuery({
    queryKey: ["global-search", query],
    queryFn: () =>
      unwrapResult(
        commands.globalSearch({
          query: query.trim(),
          resultType: null,
          includeArchived: false,
          limit: 20,
        }),
      ),
    enabled: open && query.trim().length >= 2 && !composing,
  });
  const entries = useMemo<PaletteEntry[]>(() => {
    const normalized = query.trim().toLocaleLowerCase();
    const actions = STATIC_ACTIONS.filter(
      (action) =>
        !normalized ||
        t(`palette.actions.${action.key}`).toLocaleLowerCase().includes(normalized),
    ).map((action) => ({ kind: "action" as const, ...action }));
    const results = (search.data ?? []).map((result) => ({
      kind: "result" as const,
      result,
    }));
    return [...actions, ...results];
  }, [query, search.data, t]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      const editable =
        target?.isContentEditable ||
        target?.tagName === "INPUT" ||
        target?.tagName === "TEXTAREA" ||
        target?.tagName === "SELECT";
      if (
        (event.metaKey || event.ctrlKey) &&
        event.key.toLowerCase() === "k" &&
        !editable &&
        !composing
      ) {
        event.preventDefault();
        restoreFocusRef.current = document.activeElement as HTMLElement | null;
        setOpen(true);
        return;
      }
      if (open && event.key === "Escape") {
        event.preventDefault();
        close();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  useEffect(() => {
    if (open) {
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => {
    setActiveIndex(0);
  }, [query, search.data]);

  function close() {
    setOpen(false);
    setQuery("");
    setActiveIndex(0);
    requestAnimationFrame(() => restoreFocusRef.current?.focus());
  }
  function choose(entry: PaletteEntry) {
    const path = entry.kind === "action" ? entry.path : entry.result.destination.path;
    close();
    void navigate({ to: path as never });
  }
  function onInputKeyDown(event: React.KeyboardEvent<HTMLInputElement>) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveIndex((index) => Math.min(index + 1, Math.max(entries.length - 1, 0)));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex((index) => Math.max(index - 1, 0));
    } else if (event.key === "Home") {
      event.preventDefault();
      setActiveIndex(0);
    } else if (event.key === "End") {
      event.preventDefault();
      setActiveIndex(Math.max(entries.length - 1, 0));
    } else if (event.key === "Enter" && entries[activeIndex]) {
      event.preventDefault();
      choose(entries[activeIndex]);
    }
  }

  return (
    <>
      <Button
        aria-haspopup="dialog"
        aria-label={t("palette.open")}
        className="mb-4 w-full justify-between"
        onClick={() => {
          restoreFocusRef.current = document.activeElement as HTMLElement;
          setOpen(true);
        }}
        type="button"
        variant="ghost"
      >
        <span>{t("palette.title")}</span>
        <kbd className="rounded border border-border px-1.5 py-0.5 text-xs">⌘K</kbd>
      </Button>
      {open ? (
        <div
          className="fixed inset-0 z-50 flex items-start justify-center bg-black/30 px-4 pt-[12vh]"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) close();
          }}
        >
          <section
            aria-label={t("palette.title")}
            className="w-full max-w-xl rounded-xl border border-border bg-card p-3 shadow-2xl"
            role="dialog"
          >
            <Input
              aria-autocomplete="list"
              aria-controls="command-palette-results"
              aria-expanded="true"
              aria-label={t("palette.title")}
              autoComplete="off"
              onChange={(event) => setQuery(event.target.value)}
              onCompositionEnd={() => setComposing(false)}
              onCompositionStart={() => setComposing(true)}
              onKeyDown={onInputKeyDown}
              placeholder={t("palette.placeholder")}
              ref={inputRef}
              role="combobox"
              value={query}
            />
            <div className="mt-2 flex items-center justify-between px-2 text-xs text-muted-foreground">
              <span>
                {query.trim().length >= 2 && search.isPending
                  ? t("palette.loading")
                  : query.trim().length >= 2 && search.error
                    ? t("palette.error")
                    : entries.length === 0
                      ? t("palette.empty")
                      : `${entries.length}`}
              </span>
              <span>Esc</span>
            </div>
            <div
              aria-label={t("palette.results")}
              className="mt-2 max-h-[min(60vh,28rem)] overflow-y-auto"
              id="command-palette-results"
              role="listbox"
            >
              {entries.map((entry, index) => (
                <PaletteOption
                  active={index === activeIndex}
                  entry={entry}
                  index={index}
                  key={
                    entry.kind === "action"
                      ? entry.key
                      : `${entry.result.resultType}-${entry.result.id}`
                  }
                  onChoose={() => choose(entry)}
                  t={t}
                />
              ))}
            </div>
          </section>
        </div>
      ) : null}
    </>
  );
}

function PaletteOption({
  entry,
  active,
  index,
  onChoose,
  t,
}: {
  entry: PaletteEntry;
  active: boolean;
  index: number;
  onChoose: () => void;
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  const label =
    entry.kind === "action" ? t(`palette.actions.${entry.key}`) : entry.result.label;
  return (
    <div
      aria-selected={active}
      className={`cursor-pointer rounded-lg px-3 py-2 text-sm ${active ? "bg-accent text-accent-foreground" : "hover:bg-surface-soft"}`}
      id={`palette-option-${index}`}
      onClick={onChoose}
      role="option"
    >
      <div className="flex items-center justify-between gap-2">
        <span>{label}</span>
        {entry.kind === "result" ? (
          <span className="text-xs opacity-70">
            {t("palette.type", { type: entry.result.resultType })}
            {entry.result.archived ? ` · ${t("palette.archived")}` : ""}
          </span>
        ) : null}
      </div>
      {entry.kind === "result" && entry.result.excerpt ? (
        <p className="mt-1 truncate text-xs opacity-70">{entry.result.excerpt}</p>
      ) : null}
    </div>
  );
}
