import { z } from "zod";

export const memberSchema = z.object({
  name: z.string().trim().min(1, "required").max(80, "tooLong"),
  note: z.string().max(2000, "noteTooLong"),
});

export type MemberFormValues = z.infer<typeof memberSchema>;

export const emptyMemberValues: MemberFormValues = {
  name: "",
  note: "",
};
