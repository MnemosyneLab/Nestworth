import type { TFunction } from "i18next";
import type { FieldValues, Path, UseFormReturn } from "react-hook-form";
import type { ZodIssue } from "zod";

export function translateReferenceError(
  t: TFunction,
  message: string | undefined,
): string | undefined {
  if (!message) {
    return undefined;
  }
  if (
    message === "required" ||
    message === "tooLong" ||
    message === "noteTooLong" ||
    message === "country" ||
    message === "color" ||
    message === "unsupported"
  ) {
    return t(`references.errors.${message}`);
  }
  return message;
}

export function FieldError({ message }: { message?: string }) {
  if (!message) {
    return null;
  }
  return (
    <p className="text-sm text-destructive" role="alert">
      {message}
    </p>
  );
}

export function applyZodIssues<T extends FieldValues>(
  form: UseFormReturn<T>,
  issues: ZodIssue[],
  fields: Array<Path<T>>,
) {
  const allowed = new Set(fields);
  let first: Path<T> | undefined;
  for (const issue of issues) {
    const name = issue.path[0];
    if (typeof name === "string" && allowed.has(name as Path<T>)) {
      const path = name as Path<T>;
      form.setError(path, { type: "zod", message: issue.message });
      first ??= path;
    }
  }
  if (first) {
    form.setFocus(first);
  }
}

export function applyServerFieldErrors<T extends FieldValues>(
  form: UseFormReturn<T>,
  fields: Record<string, string | undefined> | null | undefined,
  allowed: Array<Path<T>>,
) {
  if (!fields) {
    return;
  }
  const allowedFields = new Set(allowed);
  let first: Path<T> | undefined;
  for (const [field, message] of Object.entries(fields)) {
    if (message && allowedFields.has(field as Path<T>)) {
      const path = field as Path<T>;
      form.setError(path, { type: "server", message });
      first ??= path;
    }
  }
  if (first) {
    form.setFocus(first);
  }
}
