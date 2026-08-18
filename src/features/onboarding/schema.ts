import { z } from "zod";

export const onboardingSchema = z.object({
  householdName: z.string().trim().min(1, "required").max(80, "tooLong"),
  baseCurrency: z
    .string()
    .trim()
    .regex(/^[A-Z]{3}$/, "currency"),
  members: z
    .array(
      z.object({
        name: z.string().trim().min(1, "required").max(80, "tooLong"),
      }),
    )
    .min(1, "members"),
});

export type OnboardingFormValues = z.infer<typeof onboardingSchema>;

export const defaultOnboardingValues: OnboardingFormValues = {
  householdName: "",
  baseCurrency: "CNY",
  members: [{ name: "" }],
};

export function parseOnboardingStep(step: number, values: OnboardingFormValues) {
  if (step === 0) {
    return onboardingSchema.pick({ householdName: true }).safeParse(values);
  }
  if (step === 1) {
    return onboardingSchema.pick({ baseCurrency: true }).safeParse(values);
  }
  return onboardingSchema.pick({ members: true }).safeParse(values);
}
