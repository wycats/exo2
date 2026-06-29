export type PhaseRfcRelation = "driving" | "related" | "blocked";

export type PhaseRfc =
  | string
  | {
      id: string;
      target?: number | null;
      relation?: PhaseRfcRelation;
    };
