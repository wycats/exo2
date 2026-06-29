import { RTDBlock, RTDInline } from "../dom/types.js";

export class RTDBuilder {
  private blocks: RTDBlock[] = [];

  constructor() {}

  public build(): RTDBlock[] {
    return [...this.blocks];
  }

  public paragraph(text: string | RTDInline[]): this {
    this.blocks.push({
      kind: "paragraph",
      children: this.normalizeInlines(text),
    });
    return this;
  }

  public heading(
    level: 1 | 2 | 3 | 4 | 5 | 6,
    text: string | RTDInline[]
  ): this {
    this.blocks.push({
      kind: "heading",
      level,
      children: this.normalizeInlines(text),
    });
    return this;
  }

  public codeBlock(value: string, language?: string): this {
    this.blocks.push({
      kind: "code-block",
      value,
      language,
    });
    return this;
  }

  public blockquote(buildFn: (b: RTDBuilder) => void): this {
    const subBuilder = new RTDBuilder();
    buildFn(subBuilder);
    this.blocks.push({
      kind: "blockquote",
      children: subBuilder.build(),
    });
    return this;
  }

  public list(ordered: boolean, buildFn: (l: RTDListBuilder) => void): this {
    const listBuilder = new RTDListBuilder();
    buildFn(listBuilder);
    this.blocks.push({
      kind: "list",
      ordered,
      items: listBuilder.build(),
    });
    return this;
  }

  public container(variant: string, buildFn: (b: RTDBuilder) => void): this {
    const subBuilder = new RTDBuilder();
    buildFn(subBuilder);
    this.blocks.push({
      kind: "container",
      variant,
      children: subBuilder.build(),
    });
    return this;
  }

  public thematicBreak(): this {
    this.blocks.push({ kind: "thematic-break" });
    return this;
  }

  private normalizeInlines(text: string | RTDInline[]): RTDInline[] {
    if (typeof text === "string") {
      return [{ kind: "text", value: text }];
    }
    return text;
  }
}

export class RTDListBuilder {
  private items: { checked?: boolean; children: RTDBlock[] }[] = [];

  public item(buildFn: (b: RTDBuilder) => void, checked?: boolean): this {
    const subBuilder = new RTDBuilder();
    buildFn(subBuilder);
    this.items.push({
      checked,
      children: subBuilder.build(),
    });
    return this;
  }

  public build() {
    return this.items;
  }
}
