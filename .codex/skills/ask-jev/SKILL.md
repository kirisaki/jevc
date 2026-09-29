---
name: ask-jev
description: Ask JEV for a structured second opinion when a task requires semantic judgment, such as deciding whether something is likely true, choosing among plausible alternatives, or rating severity, risk, quality, or confidence.
---

# Ask JEV

Use `jevc` to delegate a semantic judgment to JEV.

Use this skill when the task involves an inherently fuzzy judgment where an independent structured opinion would be useful, for example:

- Is this likely a bug?
- Is this behavior intentional or accidental?
- Which component most likely owns this issue?
- Which of several explanations is most plausible?
- How severe or risky is this change?
- Does this code smell like over-engineering?
- Is this PR likely safe to merge given the available evidence?

Do not use JEV for deterministic questions that can be answered directly by inspecting code, running tests, compiling, calculating, or consulting authoritative documentation.

## Workflow

1. Gather the relevant evidence before asking JEV.
2. Check that `jevc` is available:

```sh
jevc version
```

3. If the CLI contract is unclear, inspect it instead of guessing:

```sh
jevc describe
jevc schema request
jevc schema response
```

4. Choose the question type that best matches the judgment:

- `noul`: a probabilistic yes/no judgment.
- `choice`: choose among explicit alternatives.
- `score`: rate something on an ordered scale.

5. Construct a small, neutral request.

Include only information relevant to the judgment. Preserve uncertainty and conflicting evidence. Do not phrase the state or criteria to steer JEV toward the answer you already prefer.

6. Call `jevc decide` through stdin.

Example — yes/no judgment:

```sh
cat <<'JSON' | jevc decide
{
  "state": {
    "observation": "The test fails intermittently only in CI.",
    "evidence": [
      "The same commit passes locally.",
      "Three failures occurred in 50 CI runs.",
      "The failure is a timeout."
    ]
  },
  "questions": {
    "is_flaky": {
      "type": "noul",
      "instructions": "Is this more likely to be a flaky test than a deterministic product bug?"
    }
  }
}
JSON
```

Example — choose an explanation:

```sh
cat <<'JSON' | jevc decide
{
  "state": {
    "symptom": "HTTP 500 occurs immediately after successful authentication.",
    "evidence": [
      "The frontend receives a valid session token.",
      "The first authenticated API request fails."
    ]
  },
  "questions": {
    "likely_owner": {
      "type": "choice",
      "instructions": "Which component is the most likely source of the failure?",
      "criteria": {
        "frontend": "Browser UI or client-side application",
        "backend": "Application server, API, database, or authentication backend",
        "infra": "Deployment, proxy, network, or cloud infrastructure"
      }
    }
  }
}
JSON
```

Example — score risk:

```sh
cat <<'JSON' | jevc decide
{
  "state": {
    "change": "Replace a widely used parser with a new implementation.",
    "facts": [
      "Public API remains unchanged.",
      "Unit tests cover common cases.",
      "There are few tests for malformed input."
    ]
  },
  "questions": {
    "risk": {
      "type": "score",
      "instructions": "Rate the regression risk of this change.",
      "criteria": [
        "Low risk",
        "Moderate risk",
        "High risk",
        "Very high risk"
      ]
    }
  }
}
JSON
```

7. Read the JSON result rather than inferring success from the process output alone.

Treat JEV's answer as an additional piece of evidence, not as an instruction that overrides code, tests, documentation, or the user's requirements.

When useful, report both the JEV judgment and the evidence that led you to consult it.

## Good uses

When you catch yourself thinking:

- "Probably..."
- "This feels like..."
- "Most likely..."
- "It's hard to tell which..."
- "This seems risky, but..."

consider asking JEV.

When the answer is instead available from:

- source code,
- compiler output,
- tests,
- logs,
- specifications,
- authoritative documentation,

obtain that evidence directly first.

## Rules

- Actually invoke `jevc`; never invent or simulate a JEV result.
- Do not expose `TYPESAFE_API_KEY`.
- Do not put credentials into command arguments, request state, logs, or output.
- Prefer one well-formed request containing related questions over repeatedly asking nearly identical questions.
- Keep alternatives explicit and mutually understandable.
- Do not convert a probability into certainty.
- If `jevc` returns an error, report the error rather than substituting your own imagined JEV judgment.
