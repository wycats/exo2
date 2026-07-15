<!-- exo:33 ulid:01kg5m2xx7s59xeecyjdkayq3y -->

# RFC 33: Print-Inspired Vertical Rhythm & Spacing

- **Supersedes**: RFC 10113




# RFC 0033: Print-Inspired Vertical Rhythm & Spacing

## Summary
Establish a rigorous "Vertical Rhythm" system for the Studio UI, inspired by print design best practices. This RFC defines specific spacing rules to ensure strong visual hierarchy, consistent grouping (Proximity), and a professional "typeset" look.

## Motivation
Current spacing is erratic.
- Lists feel disconnected (bullets too far apart).
- Labels feel floating (too far from the content they label).
- Section breaks are inconsistent.

We need a system that is **Generative** (derived from rules) rather than **Descriptive** (tweaked per element).

## Design Principles

### 1. The Law of Proximity
Things that are related should be close to each other.
- A **Label** must be closer to its **Content** than to the preceding section.
- **List Items** must be closer to each other than the List is to surrounding text.
- **Headers** must be closer to their body text than to the preceding section.

### 2. The Base Unit
We define a base spacing unit `$space` (e.g., `1em` or `1rem`). All spacing is a multiple of this unit.
- **Body Line Height**: `1.5`
- **Paragraph Spacing**: `1em` (Standard flow)

### 3. The Rules

#### Headings
Headings act as section dividers. They need significant "breathing room" above to signal a new topic, and tight spacing below to bind them to their content.
- **H1**: Top `2em`, Bottom `0.5em`
- **H2**: Top `1.5em`, Bottom `0.5em`
- **H3+**: Top `1.25em`, Bottom `0.5em`

#### Lists
Lists are cohesive units. The items within them are siblings.
- **List Item (`li`)**: Bottom margin `0.25em` (Tight).
- **List Container (`ul/ol`)**: Bottom margin `1em` (Flow).
- **Nested Lists**: Top/Bottom margin `0` (Inherit flow).

#### Label Paragraphs (The "Layout" Case)
A paragraph that immediately precedes a list or code block often acts as a label. It should be visually distinct (bold) and physically attached to the object it labels.
- **Selector**: `p:has(+ ul), p:has(+ pre)`
- **Weight**: `600` (Bold)
- **Top Margin**: `1.5em` (New subsection) -> *Revised to `1.25em` to be less aggressive*
- **Bottom Margin**: `0.25em` (Ultra-tight binding to the list)

#### Code Blocks
- **Container**: Bottom margin `1.5em` (Distinct block).

## Implementation Plan
Update `RTDRenderer.svelte` to implement these CSS rules using `:global()` selectors.
