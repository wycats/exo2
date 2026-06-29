# RTD Markup Language (RTML)

**Version:** 1.0.0 (Draft)
**Status:** Proposal
**Target:** Exosuit RTD Package

## 1. Introduction

The **RTD Markup Language (RTML)** is an HTML5-compatible serialization format for the **Rich Text DOM (RTD)**. It allows RTD structures to be represented using standard HTML syntax, making them easy to author, inspect, and parse using standard web tools.

RTML is designed to be a **strict subset** of HTML5. Any valid RTML document is a valid HTML5 document, but not all HTML5 documents are valid RTML.

## 2. Design Principles

1.  **HTML Compatibility**: The syntax MUST be parsable by any standard HTML5 parser (e.g., browser DOMParser, `cheerio`, `jsdom`).
2.  **Semantic Mapping**: HTML tags are chosen to match the semantics of RTD nodes as closely as possible.
3.  **Custom Elements**: Where standard HTML lacks a direct equivalent (e.g., `command`, `icon`), Custom Elements (kebab-case tags) are used.
4.  **Strict Whitelist**: Only tags and attributes explicitly defined in this specification are allowed. All others MUST be stripped or rejected by the parser.

## 3. The Element Map

The following table defines the normative mapping between RTD Nodes and RTML Elements.

### 3.1 Block Elements

| RTD Node      | RTML Tag        | Attributes     | Content Model        |
| :------------ | :-------------- | :------------- | :------------------- |
| `paragraph`   | `<p>`           | -              | Phrasing Content     |
| `heading`     | `<h1>` - `<h3>` | -              | Phrasing Content     |
| `blockquote`  | `<blockquote>`  | -              | Flow Content         |
| `code-block`  | `<pre>`         | `data-lang`    | Text (Source Code)   |
| `list`        | `<ul>`, `<ol>`  | -              | `<li>` elements only |
| `list-item`\* | `<li>`          | `data-checked` | Flow Content         |

_\*Note: `list-item` is not a standalone RTD Node but a structural component of the `list` node._

### 3.2 Inline Elements (Phrasing Content)

| RTD Node    | RTML Tag        | Attributes      | Content Model            |
| :---------- | :-------------- | :-------------- | :----------------------- |
| `text`      | `#text`         | -               | -                        |
| `strong`    | `<strong>`      | -               | Phrasing Content         |
| `emphasis`  | `<em>`          | -               | Phrasing Content         |
| `code-span` | `<code>`        | -               | Text                     |
| `link`      | `<a>`           | `href`, `title` | Phrasing Content         |
| `icon`      | `<rtd-icon>`    | `name`          | Empty                    |
| `command`   | `<rtd-command>` | `id`, `args`    | Phrasing Content (Label) |

## 4. Detailed Specification

### 4.1 Blocks

#### Paragraph (`<p>`)

Represents a block of text.

```html
<p>This is a paragraph.</p>
```

#### Heading (`<h1>` - `<h3>`)

Represents a section heading. The level corresponds directly to the tag name.

```html
<h2>Section Title</h2>
```

#### Blockquote (`<blockquote>`)

Represents a nested block context.

```html
<blockquote>
  <p>This is a quote.</p>
</blockquote>
```

#### Code Block (`<pre>`)

Represents a block of preformatted code.

- **Attribute `data-lang`**: (Optional) Specifies the language identifier.
- **Content**: The text content of the `<pre>` element is taken as the code value. HTML entities MUST be decoded.

```html
<pre data-lang="typescript">
const x = 1;
</pre>
```

#### List (`<ul>`, `<ol>`)

Represents a list of items.

- `<ul>`: Unordered list (`ordered: false`).
- `<ol>`: Ordered list (`ordered: true`).
- **Content**: MUST contain only `<li>` elements.

#### List Item (`<li>`)

Represents an item in a list.

- **Attribute `data-checked`**: (Optional) If present, indicates a task list item. Values: `"true"`, `"false"`.
- **Content**: Flow Content (Blocks).
  - _Normalization Rule_: If an `<li>` contains raw text or inline elements, the parser MUST wrap them in an implicit `<p>`.

```html
<ul>
  <li><p>Item 1</p></li>
  <li data-checked="true"><p>Done item</p></li>
</ul>
```

### 4.2 Inlines

#### Text (`#text`)

Represents a run of text.

#### Strong (`<strong>`)

Represents strong importance.

```html
<strong>Bold text</strong>
```

#### Emphasis (`<em>`)

Represents stress emphasis.

```html
<em>Italic text</em>
```

#### Code Span (`<code>`)

Represents a fragment of computer code.

```html
<code>console.log()</code>
```

#### Link (`<a>`)

Represents a hyperlink.

- **Attribute `href`**: (Required) The target URL (`target` property).
- **Attribute `title`**: (Optional) Advisory title (`title` property).

```html
<a href="https://example.com" title="Go to Example">Link</a>
```

#### Icon (`<rtd-icon>`)

Represents a VS Code Theme Icon.

- **Attribute `name`**: (Required) The icon name (without `$()` wrapper).

```html
<rtd-icon name="gear"></rtd-icon>
```

#### Command (`<rtd-command>`)

Represents a command trigger.

- **Attribute `id`**: (Required) The command identifier.
- **Attribute `args`**: (Optional) JSON-serialized array of arguments.

```html
<rtd-command id="workbench.action.reloadWindow">Reload</rtd-command>
<rtd-command id="my.command" args='["arg1", 123]'>Run</rtd-command>
```

## 5. Parsing Algorithm

To convert an HTML DOM tree (parsed from RTML) into an RTD tree:

1.  **Traverse** the DOM tree.
2.  **Match** each Element against the Element Map.
    - If the tag is not in the map, **unwrap** it (replace with its children) or **discard** it (if it's a script/style/meta tag).
    - If an attribute is not in the map, **ignore** it.
3.  **Normalize** content models:
    - If a Block container (e.g., `<blockquote>`, `<li>`) contains text nodes or inline elements, wrap them in a `<p>`.
    - If a `<code>` element contains child elements, extract the text content recursively.
4.  **Validate** constraints:
    - Ensure `<ul>`/`<ol>` only contain `<li>`.
    - Ensure `args` in `<rtd-command>` is valid JSON.

## 6. Serialization Algorithm

To convert an RTD tree into RTML:

1.  **Traverse** the RTD tree.
2.  **Emit** the corresponding HTML tag.
3.  **Escape** text content (standard HTML entity encoding).
4.  **Serialize** attributes:
    - `args` MUST be JSON-serialized and attribute-escaped.
