import type { CommandError, ErrorCode } from "@/generated/tauri-bindings";

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
