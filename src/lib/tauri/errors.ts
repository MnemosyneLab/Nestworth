import type { CommandError, ErrorCode } from "@/generated/tauri-bindings";

type Translate = (key: string, options?: Record<string, unknown>) => string;

export function isCommandError(error: unknown): error is CommandError {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error &&
    typeof (error as CommandError).code === "string" &&
    typeof (error as CommandError).message === "string"
  );
}

export function commandErrorFromUnknown(error: unknown): CommandError {
  if (isCommandError(error)) {
    return error;
  }
  return {
    code: "INTERNAL_ERROR" satisfies ErrorCode,
    message: "An internal application error occurred.",
    fields: null,
  };
}

export function formatCommandError(t: Translate, error: CommandError): string {
  const reason = error.fields?.reason;
  if (reason) {
    return t(`errors.reasons.${reason}`, { defaultValue: error.message });
  }
  return t(`errors.${error.code}`, { defaultValue: error.message });
}

export async function unwrapResult<T>(
  result: Promise<{ status: "ok"; data: T } | { status: "error"; error: CommandError }>,
): Promise<T> {
  const resolved = await result;
  if (resolved.status === "error") {
    throw resolved.error;
  }
  return resolved.data;
}
