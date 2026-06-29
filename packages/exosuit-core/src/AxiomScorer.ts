export interface Axiom {
  title: string;
  body: string;
  full: string;
  score?: number;
}

export class AxiomScorer {
  static parse(markdown: string): Axiom[] {
    return markdown
      .split(/^## /m)
      .slice(1)
      .map((section) => {
        const lines = section.split("\n");
        const title = lines[0].trim();
        const body = lines.slice(1).join("\n").trim();
        return { title, body, full: `## ${section}` };
      });
  }

  static score(axioms: Axiom[], query: string): Axiom[] {
    const queryTokens = query
      .toLowerCase()
      .split(/\s+/)
      .filter((t) => t.length > 3);
    if (queryTokens.length === 0) {
      return [];
    }

    const scoredAxioms = axioms.map((axiom) => {
      let score = 0;
      const text = (axiom.title + " " + axiom.body).toLowerCase();
      for (const token of queryTokens) {
        if (text.includes(token)) {
          score += 1;
        }
      }
      return { ...axiom, score };
    });

    return scoredAxioms
      .filter((a) => a.score! > 0)
      .sort((a, b) => b.score! - a.score!);
  }
}
