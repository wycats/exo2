#!/bin/bash
set -e

echo "=== Running Verification ==="
exo verify

echo "=== Coherence Checkpoint ==="
echo "Please manually verify the following documents for alignment with the code:"
echo "1. [Plan] exo task list / exo goal list"
echo "2. [RFCs] docs/rfcs/* (if you made or relied on a decision)"
echo ""
echo "Check for:"
echo "- Are task and goal outcomes recorded through exo commands?"
echo "- If you made a new decision, did you capture it as an RFC change?"

echo "=== Verification Successful ==="
