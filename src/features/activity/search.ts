export type ActivitySearch = {
  accountId?: string;
  instrumentId?: string;
  kind?: string;
  classification?: string;
  start?: string;
  end?: string;
  cursor?: string;
};

const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

export function validateActivitySearch(
  search: Record<string, unknown>,
): ActivitySearch {
  const next: ActivitySearch = {};
  const accountId = readString(search.accountId);
  const instrumentId = readString(search.instrumentId);
  const kind = readString(search.kind);
  const classification = readString(search.classification);
  const start = readDate(search.start);
  const end = readDate(search.end);
  const cursor = readString(search.cursor);
  if (accountId) {
    next.accountId = accountId;
  }
  if (instrumentId) {
    next.instrumentId = instrumentId;
  }
  if (kind) {
    next.kind = kind;
  }
  if (classification) {
    next.classification = classification;
  }
  if (start) {
    next.start = start;
  }
  if (end) {
    next.end = end;
  }
  if (cursor) {
    next.cursor = cursor;
  }
  return next;
}

export function mergeActivitySearch(
  prev: ActivitySearch | Record<string, unknown>,
  patch: Partial<ActivitySearch>,
): ActivitySearch {
  return validateActivitySearch({ ...prev, ...patch });
}

function readString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function readDate(value: unknown): string | undefined {
  const text = readString(value);
  return text && DATE_PATTERN.test(text) ? text : undefined;
}
