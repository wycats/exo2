import { z } from "zod";

const RawAxiomSchema = z
  .object({
    id: z.string(),

    // Canonical fields
    principle: z.string().optional(),
    rationale: z.string().optional(),
    implications: z.array(z.string()).optional(),
    notes: z.string().optional(),

    // Legacy / drift fields
    title: z.string().optional(),
    content: z.string().optional(),
    statement: z.string().optional(),
    why: z.string().optional(),
    implication: z.union([z.string(), z.array(z.string())]).optional(),

    tags: z.array(z.string()).optional(),
  })
  .transform((raw) => {
    const principle = raw.principle || raw.title || raw.id;
    const rationale = raw.rationale || raw.why;

    let implications: string[] = [];
    if (Array.isArray(raw.implications)) {
      implications = raw.implications;
    } else if (typeof raw.implication === "string") {
      implications = [raw.implication];
    } else if (Array.isArray(raw.implication)) {
      implications = raw.implication;
    }

    const notes = raw.notes || raw.content || raw.statement;

    return {
      id: raw.id,
      principle,
      rationale,
      implications,
      notes,
      tags: raw.tags,
    };
  });

export const AxiomSchema = RawAxiomSchema;

export type Axiom = z.infer<typeof AxiomSchema>;

export const AxiomFileSchema = z.object({
  axioms: z.array(AxiomSchema).default([]),
});

export type AxiomFile = z.infer<typeof AxiomFileSchema>;
