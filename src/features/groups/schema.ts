import { z } from "zod";

export const GROUP_ICONS = [
  "wallet",
  "home",
  "shield",
  "briefcase",
  "heart",
  "star",
] as const;

export const GROUP_COLORS = [
  "#2563EB",
  "#16A34A",
  "#DC2626",
  "#D97706",
  "#7C3AED",
  "#0F766E",
] as const;

export const groupSchema = z.object({
  name: z.string().trim().min(1, "required").max(80, "tooLong"),
  iconKey: z.string().max(80, "tooLong"),
  color: z
    .string()
    .trim()
    .refine((value) => value === "" || /^#[0-9A-Fa-f]{6}$/.test(value), "color"),
  description: z.string().max(2000, "noteTooLong"),
});

export type GroupFormValues = z.infer<typeof groupSchema>;

export const emptyGroupValues: GroupFormValues = {
  name: "",
  iconKey: "",
  color: "",
  description: "",
};
