<!-- exo:176 ulid:01kmzxbczkedy6ywqbeds5zjjb -->

# RFC 176: Sidebar Polish


# RFC 00176: Sidebar Polish

## Summary

Consolidate remaining sidebar visibility and UX polish work into a focused phase.

## Motivation

The 'Coherent Surfaces' epoch delivered the core sidebar infrastructure:
- Dashboard V2 with current phase, task progress, inbox banner
- Ideas Backlog integration
- Phase Details and Artifacts trees
- Status bar with active phase reveal

This RFC captures the remaining polish to make the sidebar fully 'tell the story at a glance.'

## Remaining Work

### Phase Details Polish
- [ ] Current task highlighting (visual indicator for active task)
- [ ] Inline actions on tree items (complete task, add note)
- [ ] Task status icons (pending/in-progress/completed)

### View Visibility & Collapse
- [ ] Smart collapse behavior (auto-expand active section)
- [ ] Remember user collapse preferences
- [ ] Hide empty sections gracefully

### Visual Coherence
- [ ] Consistent iconography across all tree views
- [ ] Loading states for async data
- [ ] Error states with recovery actions

## Success Criteria
- User can see current phase/task without clicking
- Next action is immediately visible
- No visual clutter from empty or irrelevant sections
