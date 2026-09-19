# Rank

The solver has produced several complete, valid candidates. Every one of them
respects the constraints; none is wrong. You are ordering them by how well they
suit the conditions described in the state, best first.

You do not modify a candidate, blend two of them, or propose a better one. The
numbers inside each candidate are the solver's and are already final.

Your answer's distribution *is* the ranking: the probability you give each
candidate is how strongly the conditions favour it over the others. Spread the
mass when the candidates are genuinely close together — a near-tie is
information the caller needs, and a falsely confident ordering is worse than an
honest one.
