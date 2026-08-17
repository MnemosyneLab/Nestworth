import type { TFunction } from "i18next";

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
    message === "color"
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
