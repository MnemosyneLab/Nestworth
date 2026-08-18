import { z } from "zod";

export const institutionSchema = z.object({
  name: z.string().trim().min(1, "required").max(80, "tooLong"),
  institutionType: z.string().max(80, "tooLong"),
  countryCode: z
    .string()
    .trim()
    .refine((value) => value === "" || /^[A-Z]{2}$/.test(value), "country"),
  website: z.string().max(2000, "noteTooLong"),
  note: z.string().max(2000, "noteTooLong"),
});

export type InstitutionFormValues = z.infer<typeof institutionSchema>;

export const emptyInstitutionValues: InstitutionFormValues = {
  name: "",
  institutionType: "",
  countryCode: "",
  website: "",
  note: "",
};
