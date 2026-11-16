# Local AI files

Frokla's Rust server runs ONNX models locally with one inference thread. The
tracked starter image model lives in `models/base-nsfw-384/`; it is a general
NSFW detector and only adds a review hint. It never rejects a drawing by itself.

`datasets/` contains a small, attributed Quick, Draw! sketch sample. Review the
dataset notes before using it. It is not a set of rejected content and cannot
represent Frokla's full moderation policy.

While serving, Rust recalculates the current drawing model's review threshold
after every moderator decision on a scored drawing. It can make a small,
conservative adjustment as soon as the first decision is recorded, then uses both
approved and rejected examples to improve calibration as they accumulate. This
uses little memory and affects future hints only; moderators still make every
final decision. It does not update ONNX weights on the live server.

For later training, copy images that a moderator has checked into the ignored
folders `dataset/allowed/` and `dataset/rejected/`. Or export whole drawings
that moderators already approved or rejected from the site's SQLite database:

```sh
cargo run --release -- export-training /tmp/frokla-decisions.jsonl
```

Train on a Mac or separate workstation, not beside the live server on the
4-GB Ubuntu machine:

```sh
python3 -m venv .venv
.venv/bin/pip install -r local_ai/requirements.txt
.venv/bin/python local_ai/train_vision.py --export /tmp/frokla-decisions.jsonl
```

The trainer fine-tunes the final ViT blocks of Marqo's Apache-2.0 NSFW model,
checks its ONNX export with ONNX Runtime, then switches the local ignored
`models/current.json` pointer. Training needs at least 20 labeled examples per
class and downloads the original PyTorch weights on its first run. Keep the
validation score as a small-dataset diagnostic, not a quality guarantee. Human
moderation remains authoritative.

Training images, database exports, trained model versions, and the pointer are
excluded from Git. Copy a trained version and `current.json` separately when
installing that trained model on Ubuntu. The SQLite file itself stays at its
existing `FROKLA_DB` path and is never part of model deployment.
