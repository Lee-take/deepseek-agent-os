# Step 0 T1 Offline Baseline

Status: **COMPLETED** — five independent clean-state runs passed all six deterministic verifiers and the C0B completion gate.

## Scope and environment

- Runner: `ds-agent.step-0-offline-runner/v1`
- App version: `1.0.2`
- Source base commit: `9d37c1f168cd2621a5506a17feb3b9bd2057851a`
- Release tag: `v1.0.2`
- Environment: `windows` / `x86_64` / `windows-offline-synthetic`
- Fixture mode: `synthetic-deterministic-offline`
- Renderer mode: `deterministic-receipt-fixture-no-office-or-poppler`
- Scope disclosure: Step 0 only: five offline synthetic T1 runs used fresh explicit roots. Files alone never imply completion; receipts and all six deterministic verifiers jointly gate completion. No later product capability was started or claimed

## Contract binding

- Task: `t1-monthly-operations-brief` revision 1
- Prompt: Summarize the specified synthetic Excel, Word, and PDF inputs into a reconciliation workbook and a one-page monthly operations brief, flag anomalies, and save both outputs.
- TaskSpec fingerprint: `a2504a599f8b5990904bcbc290973efa3f47a40d54cc0360c340519025f5f809`
- Fixture set: `t1-monthly-operations-brief-fixture-set-v1`
- Fixture manifest SHA-256: `a96941c5b9267c472e58d82015721b1656c2d97a7697e380ac626509d316ed38`
- Allowed capabilities: `["file_read","file_write"]`
- Expected risk: `"high"`
- Authorization budget: 1
- Required done_when/verifiers: 6

## Five-run result

| Run | Clean state | A/Q/H/S/F | Verifiers | Gate | Completion | Clarifications | Authorizations | Human interventions | Logical ms | Observed wall ms* | Tokens | Cost micro-USD | Failure stage |
| ---: | --- | --- | ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | `clean-state-01` | A | 6/6 | PASS | completed | 0 | 0 | 0 | 0 | 87.097 | 0 | 0 | none |
| 2 | `clean-state-02` | A | 6/6 | PASS | completed | 0 | 0 | 0 | 0 | 81.516 | 0 | 0 | none |
| 3 | `clean-state-03` | A | 6/6 | PASS | completed | 0 | 0 | 0 | 0 | 79.806 | 0 | 0 | none |
| 4 | `clean-state-04` | A | 6/6 | PASS | completed | 0 | 0 | 0 | 0 | 80.806 | 0 | 0 | none |
| 5 | `clean-state-05` | A | 6/6 | PASS | completed | 0 | 0 | 0 | 0 | 82.503 | 0 | 0 | none |

* Observed wall-clock values are non-normative, excluded from canonical hashing, and never affect PASS.

## Verifier outcomes and evidence

| Run | done_when | Verifier | Status | Evidence kind | Evidence ref | Evidence SHA-256 |
| ---: | --- | --- | --- | --- | --- | --- |
| 1 | `source-manifest` | `t1.source-manifest/v1` | Passed | `source_manifest` | `benchmark-evidence:t1-source-manifest` | `ad3d864c524dae21cb0798f6b6ce98f8a603d7dc49fd943c05637bd9b2f9bba6` |
| 1 | `fact-provenance` | `t1.provenance/v1` | Passed | `fact_provenance` | `benchmark-evidence:t1-fact-provenance` | `4394002eae973577efc10ff98cbaa05efe758198eb9f3dff876edba208300a2a` |
| 1 | `reconciliation-xlsx` | `t1.reconciliation-xlsx/v1` | Passed | `reconciliation_xlsx` | `outputs/t1-reconciliation.xlsx` | `6ee477e0ca940cbf2cecd572ce9842e2871f9cba3e8d1d8831b88224294901f1` |
| 1 | `one-page-brief` | `t1.one-page-pptx/v1` | Passed | `one_page_pptx` | `outputs/t1-monthly-brief.pptx` | `420dc8f1faa926c71f99a7c8f3aaa8f0b2160af77548d16b715ceff4da4373f4` |
| 1 | `actual-render` | `t1.actual-render/v1` | Passed | `actual_render_receipt` | `benchmark-evidence:t1-actual-render` | `9863027063f52965894fe8631c297c3a30e4fd19ec3622acbb53dd13b10d6751` |
| 1 | `result-receipt` | `t1.result-receipt/v1` | Passed | `result_receipt` | `benchmark-evidence:t1-result-receipt` | `ac61029bd3a81134bf9420af7bfba35d6aa53a252b342b3236bf2e91f0d060ba` |
| 2 | `source-manifest` | `t1.source-manifest/v1` | Passed | `source_manifest` | `benchmark-evidence:t1-source-manifest` | `ad3d864c524dae21cb0798f6b6ce98f8a603d7dc49fd943c05637bd9b2f9bba6` |
| 2 | `fact-provenance` | `t1.provenance/v1` | Passed | `fact_provenance` | `benchmark-evidence:t1-fact-provenance` | `4394002eae973577efc10ff98cbaa05efe758198eb9f3dff876edba208300a2a` |
| 2 | `reconciliation-xlsx` | `t1.reconciliation-xlsx/v1` | Passed | `reconciliation_xlsx` | `outputs/t1-reconciliation.xlsx` | `6ee477e0ca940cbf2cecd572ce9842e2871f9cba3e8d1d8831b88224294901f1` |
| 2 | `one-page-brief` | `t1.one-page-pptx/v1` | Passed | `one_page_pptx` | `outputs/t1-monthly-brief.pptx` | `420dc8f1faa926c71f99a7c8f3aaa8f0b2160af77548d16b715ceff4da4373f4` |
| 2 | `actual-render` | `t1.actual-render/v1` | Passed | `actual_render_receipt` | `benchmark-evidence:t1-actual-render` | `9863027063f52965894fe8631c297c3a30e4fd19ec3622acbb53dd13b10d6751` |
| 2 | `result-receipt` | `t1.result-receipt/v1` | Passed | `result_receipt` | `benchmark-evidence:t1-result-receipt` | `ab9d39dcb8d32df8cd80379745d583a03f3c26bff022240a8434e70790e651dc` |
| 3 | `source-manifest` | `t1.source-manifest/v1` | Passed | `source_manifest` | `benchmark-evidence:t1-source-manifest` | `ad3d864c524dae21cb0798f6b6ce98f8a603d7dc49fd943c05637bd9b2f9bba6` |
| 3 | `fact-provenance` | `t1.provenance/v1` | Passed | `fact_provenance` | `benchmark-evidence:t1-fact-provenance` | `4394002eae973577efc10ff98cbaa05efe758198eb9f3dff876edba208300a2a` |
| 3 | `reconciliation-xlsx` | `t1.reconciliation-xlsx/v1` | Passed | `reconciliation_xlsx` | `outputs/t1-reconciliation.xlsx` | `6ee477e0ca940cbf2cecd572ce9842e2871f9cba3e8d1d8831b88224294901f1` |
| 3 | `one-page-brief` | `t1.one-page-pptx/v1` | Passed | `one_page_pptx` | `outputs/t1-monthly-brief.pptx` | `420dc8f1faa926c71f99a7c8f3aaa8f0b2160af77548d16b715ceff4da4373f4` |
| 3 | `actual-render` | `t1.actual-render/v1` | Passed | `actual_render_receipt` | `benchmark-evidence:t1-actual-render` | `9863027063f52965894fe8631c297c3a30e4fd19ec3622acbb53dd13b10d6751` |
| 3 | `result-receipt` | `t1.result-receipt/v1` | Passed | `result_receipt` | `benchmark-evidence:t1-result-receipt` | `3499252496c827c4cb481b0533f16d8816b7f902b0f4a57653426523a673f9a7` |
| 4 | `source-manifest` | `t1.source-manifest/v1` | Passed | `source_manifest` | `benchmark-evidence:t1-source-manifest` | `ad3d864c524dae21cb0798f6b6ce98f8a603d7dc49fd943c05637bd9b2f9bba6` |
| 4 | `fact-provenance` | `t1.provenance/v1` | Passed | `fact_provenance` | `benchmark-evidence:t1-fact-provenance` | `4394002eae973577efc10ff98cbaa05efe758198eb9f3dff876edba208300a2a` |
| 4 | `reconciliation-xlsx` | `t1.reconciliation-xlsx/v1` | Passed | `reconciliation_xlsx` | `outputs/t1-reconciliation.xlsx` | `6ee477e0ca940cbf2cecd572ce9842e2871f9cba3e8d1d8831b88224294901f1` |
| 4 | `one-page-brief` | `t1.one-page-pptx/v1` | Passed | `one_page_pptx` | `outputs/t1-monthly-brief.pptx` | `420dc8f1faa926c71f99a7c8f3aaa8f0b2160af77548d16b715ceff4da4373f4` |
| 4 | `actual-render` | `t1.actual-render/v1` | Passed | `actual_render_receipt` | `benchmark-evidence:t1-actual-render` | `9863027063f52965894fe8631c297c3a30e4fd19ec3622acbb53dd13b10d6751` |
| 4 | `result-receipt` | `t1.result-receipt/v1` | Passed | `result_receipt` | `benchmark-evidence:t1-result-receipt` | `8ad50246cdd041c2f1e0224d4b166f39e23e15058a7ab27c723cf49ba134ee74` |
| 5 | `source-manifest` | `t1.source-manifest/v1` | Passed | `source_manifest` | `benchmark-evidence:t1-source-manifest` | `ad3d864c524dae21cb0798f6b6ce98f8a603d7dc49fd943c05637bd9b2f9bba6` |
| 5 | `fact-provenance` | `t1.provenance/v1` | Passed | `fact_provenance` | `benchmark-evidence:t1-fact-provenance` | `4394002eae973577efc10ff98cbaa05efe758198eb9f3dff876edba208300a2a` |
| 5 | `reconciliation-xlsx` | `t1.reconciliation-xlsx/v1` | Passed | `reconciliation_xlsx` | `outputs/t1-reconciliation.xlsx` | `6ee477e0ca940cbf2cecd572ce9842e2871f9cba3e8d1d8831b88224294901f1` |
| 5 | `one-page-brief` | `t1.one-page-pptx/v1` | Passed | `one_page_pptx` | `outputs/t1-monthly-brief.pptx` | `420dc8f1faa926c71f99a7c8f3aaa8f0b2160af77548d16b715ceff4da4373f4` |
| 5 | `actual-render` | `t1.actual-render/v1` | Passed | `actual_render_receipt` | `benchmark-evidence:t1-actual-render` | `9863027063f52965894fe8631c297c3a30e4fd19ec3622acbb53dd13b10d6751` |
| 5 | `result-receipt` | `t1.result-receipt/v1` | Passed | `result_receipt` | `benchmark-evidence:t1-result-receipt` | `3a29ff15824449135c07c8dd486c650ebd5c4e68589fe619eef56e3edd1ee57b` |

## Artifact, manifest, receipt, and run hashes

| Run | Kind | Relative reference | Bytes | SHA-256 |
| ---: | --- | --- | ---: | --- |
| 1 | `run_binding` | `workspace/run-binding.json` | 366 | `d7a96e13cb62021cdc9ee510b2eecb80280706bbeb268dd764e6c4af065bd46a` |
| 1 | `fixture` | `fixture/inputs/01-monthly-revenue.xlsx` | 4677 | `7877ef41911263907895adba7e3c4b18f1795bfbb6146d2b858f9cbec6d2cfa8` |
| 1 | `fixture` | `fixture/inputs/02-operations-notes.docx` | 2526 | `b8914071c772e000d7f13a7f8632808cf115700e5bd1c45232314b0280ecd391` |
| 1 | `fixture` | `fixture/inputs/03-risk-summary.pdf` | 867 | `ab4e27543100cb8a2803b14c89a35a81a5150b3393403ba68a82b737712b52d7` |
| 1 | `candidate_artifact` | `output/outputs/t1-reconciliation.xlsx` | 8356 | `6ee477e0ca940cbf2cecd572ce9842e2871f9cba3e8d1d8831b88224294901f1` |
| 1 | `candidate_artifact` | `output/outputs/t1-monthly-brief.pptx` | 5718 | `420dc8f1faa926c71f99a7c8f3aaa8f0b2160af77548d16b715ceff4da4373f4` |
| 1 | `render_preview` | `output/previews/t1-reconciliation-1.png` | 514 | `d7923e2dd54736212ad93803531069565aff392fca5d88f48eade63df4f5d447` |
| 1 | `render_preview` | `output/previews/t1-monthly-brief-1.png` | 514 | `d7923e2dd54736212ad93803531069565aff392fca5d88f48eade63df4f5d447` |
| 1 | `manifest_receipt` | `output/receipts/source-manifest.json` | 1370 | `6dc9bc1cf1f0c6f77a47ea432fce44a5c79f5e3c0668eb6b8e5d867140a304de` |
| 1 | `manifest_receipt` | `output/receipts/provenance-manifest.json` | 11611 | `3d18c6f25d67d5c48f0bee2884b50aadeaf2f3ec02ac3567d8a06e7386981c78` |
| 1 | `verification_receipt` | `output/receipts/actual-render-receipt.json` | 1392 | `211d6d98a40b09e95cfe174bae7c0e7fd843c129617363fc9633085762539b18` |
| 1 | `verification_receipt` | `output/receipts/result-receipt.json` | 1297 | `09ed940bca0948454b47f4245c7edcfe3f981553a6ddf8099c12b4d134df1327` |
| 1 | `benchmark_run_result` | `output/receipts/benchmark-run-result.json` | 3977 | `7e586814884f66068a8159b444267f907e860c89348d80c0f3a2c4d4dc0e3925` |
| 2 | `run_binding` | `workspace/run-binding.json` | 366 | `9a7b051b2bafe36d74c769b13448e46241d42659f8e66e5824a0e0d1314b8f61` |
| 2 | `fixture` | `fixture/inputs/01-monthly-revenue.xlsx` | 4677 | `7877ef41911263907895adba7e3c4b18f1795bfbb6146d2b858f9cbec6d2cfa8` |
| 2 | `fixture` | `fixture/inputs/02-operations-notes.docx` | 2526 | `b8914071c772e000d7f13a7f8632808cf115700e5bd1c45232314b0280ecd391` |
| 2 | `fixture` | `fixture/inputs/03-risk-summary.pdf` | 867 | `ab4e27543100cb8a2803b14c89a35a81a5150b3393403ba68a82b737712b52d7` |
| 2 | `candidate_artifact` | `output/outputs/t1-reconciliation.xlsx` | 8356 | `6ee477e0ca940cbf2cecd572ce9842e2871f9cba3e8d1d8831b88224294901f1` |
| 2 | `candidate_artifact` | `output/outputs/t1-monthly-brief.pptx` | 5718 | `420dc8f1faa926c71f99a7c8f3aaa8f0b2160af77548d16b715ceff4da4373f4` |
| 2 | `render_preview` | `output/previews/t1-reconciliation-1.png` | 514 | `d7923e2dd54736212ad93803531069565aff392fca5d88f48eade63df4f5d447` |
| 2 | `render_preview` | `output/previews/t1-monthly-brief-1.png` | 514 | `d7923e2dd54736212ad93803531069565aff392fca5d88f48eade63df4f5d447` |
| 2 | `manifest_receipt` | `output/receipts/source-manifest.json` | 1370 | `6dc9bc1cf1f0c6f77a47ea432fce44a5c79f5e3c0668eb6b8e5d867140a304de` |
| 2 | `manifest_receipt` | `output/receipts/provenance-manifest.json` | 11611 | `3d18c6f25d67d5c48f0bee2884b50aadeaf2f3ec02ac3567d8a06e7386981c78` |
| 2 | `verification_receipt` | `output/receipts/actual-render-receipt.json` | 1392 | `211d6d98a40b09e95cfe174bae7c0e7fd843c129617363fc9633085762539b18` |
| 2 | `verification_receipt` | `output/receipts/result-receipt.json` | 1297 | `edf75f84030f44bfbcdd0be87b7d586be89c56450b618816feca54e5ff19e2ca` |
| 2 | `benchmark_run_result` | `output/receipts/benchmark-run-result.json` | 3977 | `2500b7f672ed39e48f8c2f9bd1913beb2b246b06f904da9ceb6387b802cb53da` |
| 3 | `run_binding` | `workspace/run-binding.json` | 366 | `cc40d081f5ef8a62295a3100e575385b2ecedfd9c1c3aeb018da29c12decdaa0` |
| 3 | `fixture` | `fixture/inputs/01-monthly-revenue.xlsx` | 4677 | `7877ef41911263907895adba7e3c4b18f1795bfbb6146d2b858f9cbec6d2cfa8` |
| 3 | `fixture` | `fixture/inputs/02-operations-notes.docx` | 2526 | `b8914071c772e000d7f13a7f8632808cf115700e5bd1c45232314b0280ecd391` |
| 3 | `fixture` | `fixture/inputs/03-risk-summary.pdf` | 867 | `ab4e27543100cb8a2803b14c89a35a81a5150b3393403ba68a82b737712b52d7` |
| 3 | `candidate_artifact` | `output/outputs/t1-reconciliation.xlsx` | 8356 | `6ee477e0ca940cbf2cecd572ce9842e2871f9cba3e8d1d8831b88224294901f1` |
| 3 | `candidate_artifact` | `output/outputs/t1-monthly-brief.pptx` | 5718 | `420dc8f1faa926c71f99a7c8f3aaa8f0b2160af77548d16b715ceff4da4373f4` |
| 3 | `render_preview` | `output/previews/t1-reconciliation-1.png` | 514 | `d7923e2dd54736212ad93803531069565aff392fca5d88f48eade63df4f5d447` |
| 3 | `render_preview` | `output/previews/t1-monthly-brief-1.png` | 514 | `d7923e2dd54736212ad93803531069565aff392fca5d88f48eade63df4f5d447` |
| 3 | `manifest_receipt` | `output/receipts/source-manifest.json` | 1370 | `6dc9bc1cf1f0c6f77a47ea432fce44a5c79f5e3c0668eb6b8e5d867140a304de` |
| 3 | `manifest_receipt` | `output/receipts/provenance-manifest.json` | 11611 | `3d18c6f25d67d5c48f0bee2884b50aadeaf2f3ec02ac3567d8a06e7386981c78` |
| 3 | `verification_receipt` | `output/receipts/actual-render-receipt.json` | 1392 | `211d6d98a40b09e95cfe174bae7c0e7fd843c129617363fc9633085762539b18` |
| 3 | `verification_receipt` | `output/receipts/result-receipt.json` | 1297 | `709658f164e45eeacf659cb601760e9d78763a90f84d1162988b8d560b769f81` |
| 3 | `benchmark_run_result` | `output/receipts/benchmark-run-result.json` | 3977 | `ee3b36fad7209911bd0bcb1280441cc09593de47ec3b7112e68530712c7d0b73` |
| 4 | `run_binding` | `workspace/run-binding.json` | 366 | `0c88ce9414ab3f8d74b449174830ea508edca877177dee20d7a967fc595ddd3d` |
| 4 | `fixture` | `fixture/inputs/01-monthly-revenue.xlsx` | 4677 | `7877ef41911263907895adba7e3c4b18f1795bfbb6146d2b858f9cbec6d2cfa8` |
| 4 | `fixture` | `fixture/inputs/02-operations-notes.docx` | 2526 | `b8914071c772e000d7f13a7f8632808cf115700e5bd1c45232314b0280ecd391` |
| 4 | `fixture` | `fixture/inputs/03-risk-summary.pdf` | 867 | `ab4e27543100cb8a2803b14c89a35a81a5150b3393403ba68a82b737712b52d7` |
| 4 | `candidate_artifact` | `output/outputs/t1-reconciliation.xlsx` | 8356 | `6ee477e0ca940cbf2cecd572ce9842e2871f9cba3e8d1d8831b88224294901f1` |
| 4 | `candidate_artifact` | `output/outputs/t1-monthly-brief.pptx` | 5718 | `420dc8f1faa926c71f99a7c8f3aaa8f0b2160af77548d16b715ceff4da4373f4` |
| 4 | `render_preview` | `output/previews/t1-reconciliation-1.png` | 514 | `d7923e2dd54736212ad93803531069565aff392fca5d88f48eade63df4f5d447` |
| 4 | `render_preview` | `output/previews/t1-monthly-brief-1.png` | 514 | `d7923e2dd54736212ad93803531069565aff392fca5d88f48eade63df4f5d447` |
| 4 | `manifest_receipt` | `output/receipts/source-manifest.json` | 1370 | `6dc9bc1cf1f0c6f77a47ea432fce44a5c79f5e3c0668eb6b8e5d867140a304de` |
| 4 | `manifest_receipt` | `output/receipts/provenance-manifest.json` | 11611 | `3d18c6f25d67d5c48f0bee2884b50aadeaf2f3ec02ac3567d8a06e7386981c78` |
| 4 | `verification_receipt` | `output/receipts/actual-render-receipt.json` | 1392 | `211d6d98a40b09e95cfe174bae7c0e7fd843c129617363fc9633085762539b18` |
| 4 | `verification_receipt` | `output/receipts/result-receipt.json` | 1297 | `13effbf45f683b3fa14992868f68938303ade386a9ed2112c0120d0dacd3b10b` |
| 4 | `benchmark_run_result` | `output/receipts/benchmark-run-result.json` | 3977 | `e648d6a8aa199f33bad116ff0e3970a4c178427ce90ff5289bb62132b1fe3c0c` |
| 5 | `run_binding` | `workspace/run-binding.json` | 366 | `1d956adabfc370718deeae2cc15de748116bc97f635c01d742d031c663a4de59` |
| 5 | `fixture` | `fixture/inputs/01-monthly-revenue.xlsx` | 4677 | `7877ef41911263907895adba7e3c4b18f1795bfbb6146d2b858f9cbec6d2cfa8` |
| 5 | `fixture` | `fixture/inputs/02-operations-notes.docx` | 2526 | `b8914071c772e000d7f13a7f8632808cf115700e5bd1c45232314b0280ecd391` |
| 5 | `fixture` | `fixture/inputs/03-risk-summary.pdf` | 867 | `ab4e27543100cb8a2803b14c89a35a81a5150b3393403ba68a82b737712b52d7` |
| 5 | `candidate_artifact` | `output/outputs/t1-reconciliation.xlsx` | 8356 | `6ee477e0ca940cbf2cecd572ce9842e2871f9cba3e8d1d8831b88224294901f1` |
| 5 | `candidate_artifact` | `output/outputs/t1-monthly-brief.pptx` | 5718 | `420dc8f1faa926c71f99a7c8f3aaa8f0b2160af77548d16b715ceff4da4373f4` |
| 5 | `render_preview` | `output/previews/t1-reconciliation-1.png` | 514 | `d7923e2dd54736212ad93803531069565aff392fca5d88f48eade63df4f5d447` |
| 5 | `render_preview` | `output/previews/t1-monthly-brief-1.png` | 514 | `d7923e2dd54736212ad93803531069565aff392fca5d88f48eade63df4f5d447` |
| 5 | `manifest_receipt` | `output/receipts/source-manifest.json` | 1370 | `6dc9bc1cf1f0c6f77a47ea432fce44a5c79f5e3c0668eb6b8e5d867140a304de` |
| 5 | `manifest_receipt` | `output/receipts/provenance-manifest.json` | 11611 | `3d18c6f25d67d5c48f0bee2884b50aadeaf2f3ec02ac3567d8a06e7386981c78` |
| 5 | `verification_receipt` | `output/receipts/actual-render-receipt.json` | 1392 | `211d6d98a40b09e95cfe174bae7c0e7fd843c129617363fc9633085762539b18` |
| 5 | `verification_receipt` | `output/receipts/result-receipt.json` | 1297 | `dad438c8370e83884380d078b19319e8df2bc69d25db3628ec8ac4ea915cdaf5` |
| 5 | `benchmark_run_result` | `output/receipts/benchmark-run-result.json` | 3977 | `c5f68b6fc22d711a446fdd3be6fadc6b78866331f931b4a78ba8c91463ab09bf` |

## Aggregate metrics

- A/Q/H/S/F: **5/0/0/0/0**
- VOCR: **5/5 (100.00%)**
- False completion: **0/5 (0.00%)**
- Verifier pass ratio: **30/30 (100.00%)**
- Evidence completeness: **30/30 (100.00%)**
- Authorization budget compliance: **5/5 (100.00%)**
- Clarifications: 0 total; 0 runs with clarification
- Human intervention: 0 total; 0 runs
- Logical duration median/p95: `0` / `0` ms
- Observed wall-clock total/median/p95*: `411.727` / `81.516` / `87.097` ms
- API calls/tokens/cost: **0 / 0 / 0**; availability `none` because this runner is offline
- Guardrail violations: unauthorized 0, duplicate external write 0, authority drift 0, approval replay 0, refusal bypass 0
- Completion decision: **completed**

## Failures, exclusions, and interpretation

- Failures: none.
- Scope exclusion: Live DeepSeek and token or price telemetry were excluded; all usage and cost values are explicit zero with availability none.
- Scope exclusion: Installed DS Agent, real Office, Poppler, real accounts, VM, connectors, and external writes were excluded by C0D scope.
- Scope exclusion: Observed wall-clock timing is non-normative and excluded from canonical hashing and PASS decisions.
- Interpretation: generating files is insufficient. Each run completed only after receipts, hashes, provenance, all six deterministic verifiers, and the C0B completion gate passed.

## Integrity

- Canonical payload SHA-256: `941f78a6159028fd5ce7af3f6633d7e86bc99802afdf7fa7964f7112249a8674`
- Markdown body SHA-256: `5b91b7e55e86294de3c8226acafd81217b089d576c739181a88351153dd4cc6f`
- Machine report: `t1-baseline-v1.json`
- Human report: `t1-baseline-v1.md`
