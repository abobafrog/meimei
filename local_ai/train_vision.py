#!/usr/bin/env python3
"""Fine-tune Frokla's local image review model using complete, human-labeled drawings."""
from __future__ import annotations

import argparse
import json
import os
import random
import shutil
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
import timm
import torch
from PIL import Image, ImageDraw, ImageOps
from torch.utils.data import DataLoader, Dataset

ROOT = Path(__file__).resolve().parent
MODEL_NAME = "hf_hub:Marqo/nsfw-image-detection-384"
IMAGE_SIZE = 384
BACKGROUND = (16, 17, 20)
INK = (166, 219, 255)
SEED = 177


def render_strokes(strokes: list[dict]) -> Image.Image:
    image = Image.new("RGB", (IMAGE_SIZE, IMAGE_SIZE), BACKGROUND)
    draw = ImageDraw.Draw(image)
    for stroke in strokes:
        color = str(stroke.get("color") or "#a6dbff").lstrip("#")
        if len(color) != 6:
            color = "a6dbff"
        rgb = tuple(bytes.fromhex(color))
        width = max(1, round(float(stroke.get("width", 18)) * IMAGE_SIZE / 4800))
        paths = stroke.get("segments") or [stroke.get("points", [])]
        for points in paths:
            scaled = [(round(float(p[0]) * (IMAGE_SIZE - 1) / 4800),
                       round(float(p[1]) * (IMAGE_SIZE - 1) / 2800)) for p in points]
            if not scaled:
                continue
            if len(scaled) == 1:
                x, y = scaled[0]
                radius = max(1, width // 2)
                draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill=rgb)
            else:
                draw.line(scaled, fill=rgb, width=width, joint="curve")
                radius = max(1, width // 2)
                for x, y in (scaled[0], scaled[-1]):
                    draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill=rgb)
    return image


def load_export(path: Path) -> list[tuple[Image.Image, int]]:
    examples = []
    if not path.is_file():
        return examples
    with path.open(encoding="utf-8") as stream:
        for line_number, line in enumerate(stream, 1):
            if not line.strip():
                continue
            row = json.loads(line)
            if row.get("kind") != "drawing":
                continue
            if row.get("label") not in (0, 1) or not row.get("strokes"):
                raise ValueError(f"{path}:{line_number}: drawing needs strokes and label 0|1")
            # Frokla JSONL labels: 0=approved/safe, 1=rejected. Marqo logits: 0=NSFW, 1=SFW.
            target = 1 if row["label"] == 0 else 0
            examples.append((render_strokes(row["strokes"]), target))
    return examples


def load_quickdraw(directory: Path) -> list[tuple[Image.Image, int]]:
    examples = []
    for path in sorted(directory.glob("quickdraw_*_sample.jsonl")):
        examples.extend(load_export(path))
    return examples


def load_image_dirs(directory: Path) -> list[tuple[Image.Image, int]]:
    examples = []
    for folder, target in (("rejected", 0), ("allowed", 1)):
        image_dir = directory / folder
        image_dir.mkdir(parents=True, exist_ok=True)
        for path in sorted(image_dir.rglob("*")):
            if not path.is_file() or path.suffix.lower() not in {".png", ".jpg", ".jpeg", ".webp", ".bmp"}:
                continue
            try:
                with Image.open(path) as source:
                    image = ImageOps.fit(source.convert("RGB"), (IMAGE_SIZE, IMAGE_SIZE), method=Image.Resampling.BICUBIC)
                    examples.append((image.copy(), target))
            except Exception as error:
                print(f"Skipping unreadable image {path}: {error}")
    return examples


class Images(Dataset):
    def __init__(self, examples, indices, training=False):
        self.examples = examples
        self.indices = indices
        self.training = training

    def __len__(self):
        return len(self.indices)

    def __getitem__(self, index):
        image, target = self.examples[self.indices[index]]
        if self.training and random.random() < 0.5:
            image = ImageOps.mirror(image)
        array = np.asarray(image, dtype=np.float32).copy() / 127.5 - 1.0
        return torch.from_numpy(array.transpose(2, 0, 1)), torch.tensor(target, dtype=torch.long)


def split_by_class(examples):
    rng = random.Random(SEED)
    train, validation = [], []
    for target in (0, 1):
        group = [index for index, (_, label) in enumerate(examples) if label == target]
        rng.shuffle(group)
        if len(group) < 20:
            class_name = "rejected" if target == 0 else "allowed"
            raise ValueError(f"need at least 20 distinct {class_name} examples; found {len(group)}")
        validation_count = max(4, len(group) // 5)
        validation.extend(group[:validation_count])
        train.extend(group[validation_count:])
    return train, validation


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--export", type=Path, help="moderator decisions from `cargo run --release -- export-training FILE.jsonl`")
    parser.add_argument("--data", type=Path, default=ROOT / "dataset", help="private allowed/ and rejected/ image folders")
    parser.add_argument("--output", type=Path, default=ROOT / "models", help="model output directory")
    parser.add_argument("--epochs", type=int, default=4)
    parser.add_argument("--batch-size", type=int, default=2)
    parser.add_argument("--learning-rate", type=float, default=1e-5)
    args = parser.parse_args()
    if args.epochs < 1 or args.epochs > 30 or not 1 <= args.batch_size <= 16:
        parser.error("epochs must be 1..30 and batch-size 1..16")

    random.seed(SEED)
    np.random.seed(SEED)
    torch.manual_seed(SEED)
    torch.set_num_threads(max(1, min(2, os.cpu_count() or 1)))
    examples = load_quickdraw(ROOT / "datasets")
    examples.extend(load_image_dirs(args.data))
    if args.export:
        examples.extend(load_export(args.export))
    if not examples:
        raise SystemExit("No drawings found. Add reviewed examples or use the bundled QuickDraw samples.")
    train_indices, validation_indices = split_by_class(examples)
    train_loader = DataLoader(Images(examples, train_indices, training=True), batch_size=args.batch_size, shuffle=True, num_workers=0)
    validation_loader = DataLoader(Images(examples, validation_indices), batch_size=args.batch_size, shuffle=False, num_workers=0)

    if torch.cuda.is_available():
        device = torch.device("cuda")
    elif torch.backends.mps.is_available():
        device = torch.device("mps")
    else:
        device = torch.device("cpu")
    model = timm.create_model(MODEL_NAME, pretrained=True)
    model.eval()
    data_config = timm.data.resolve_model_data_config(model)
    if tuple(data_config["input_size"]) != (3, IMAGE_SIZE, IMAGE_SIZE) or any(abs(float(v) - 0.5) > 1e-6 for v in data_config["mean"] + data_config["std"]):
        raise RuntimeError("base model preprocessing changed; expected 384x384 RGB normalized with mean/std 0.5")
    for parameter in model.parameters():
        parameter.requires_grad_(False)
    for block in model.blocks[-2:]:
        for parameter in block.parameters():
            parameter.requires_grad_(True)
    for layer_name in ("norm", "head"):
        for parameter in getattr(model, layer_name).parameters():
            parameter.requires_grad_(True)
    model.to(device)
    optimizer = torch.optim.AdamW((p for p in model.parameters() if p.requires_grad), lr=args.learning_rate, weight_decay=0.01)
    loss_fn = torch.nn.CrossEntropyLoss()

    for epoch in range(args.epochs):
        model.train()
        total_loss = 0.0
        batches = 0
        for images, targets in train_loader:
            images, targets = images.to(device), targets.to(device)
            optimizer.zero_grad(set_to_none=True)
            loss = loss_fn(model(images), targets)
            loss.backward()
            optimizer.step()
            total_loss += float(loss.detach().cpu())
            batches += 1
        print(f"epoch {epoch + 1}/{args.epochs}: loss={total_loss / max(1, batches):.4f}")

    model.eval()
    correct = total = 0
    with torch.inference_mode():
        for images, targets in validation_loader:
            logits = model(images.to(device))
            predictions = logits.argmax(dim=1).cpu()
            correct += int((predictions == targets).sum())
            total += len(targets)
    validation_accuracy = correct / max(1, total)
    model.to("cpu")
    model.eval()

    version = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    version_dir = output / version
    version_dir.mkdir(mode=0o700)
    temporary_onnx = version_dir / ".drawing.float.onnx"
    try:
        example = torch.zeros((1, 3, IMAGE_SIZE, IMAGE_SIZE), dtype=torch.float32)
        torch.onnx.export(model, example, str(temporary_onnx), input_names=["pixel_values"], output_names=["logits"], dynamic_axes={"pixel_values": {0: "batch"}, "logits": {0: "batch"}}, opset_version=17, dynamo=False)
        import onnx
        import onnxruntime as ort
        onnx.checker.check_model(str(temporary_onnx))
        session = ort.InferenceSession(str(temporary_onnx), providers=["CPUExecutionProvider"])
        with torch.inference_mode():
            expected = model(example).numpy()
        actual = session.run(["logits"], {"pixel_values": example.numpy()})[0]
        np.testing.assert_allclose(actual, expected, rtol=1e-3, atol=1e-4)
        os.replace(temporary_onnx, version_dir / "drawing.onnx")

        pointer = output / "current.json"
        previous = None
        if pointer.exists():
            try:
                previous = json.loads(pointer.read_text(encoding="utf-8")).get("directory")
            except (OSError, json.JSONDecodeError):
                pass
        if isinstance(previous, str) and "/" not in previous and "\\" not in previous:
            old_text = output / previous / "text.onnx"
            if old_text.is_file():
                shutil.copy2(old_text, version_dir / "text.onnx")
        metadata = {
            "version": version,
            "created_utc": datetime.now(timezone.utc).isoformat(),
            "source_model": MODEL_NAME,
            "drawing_input": "pixel_values",
            "drawing_size": IMAGE_SIZE,
            "drawing_normalize": "minus_one_to_one",
            "drawing_reject_index": 0,
            "labels": ["nsfw", "sfw"],
            "training_examples": len(train_indices),
            "validation_examples": len(validation_indices),
            "validation_accuracy": validation_accuracy,
            "decision": "assist_review_only",
            "note": "Validation accuracy is only on this small local split; human decisions remain authoritative.",
        }
        (version_dir / "model.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2), encoding="utf-8")
        temporary_pointer = output / f".current-{version}.tmp"
        temporary_pointer.write_text(json.dumps({"directory": version}, indent=2), encoding="utf-8")
        os.replace(temporary_pointer, pointer)
        print(json.dumps(metadata, ensure_ascii=False, indent=2))
    except Exception:
        shutil.rmtree(version_dir, ignore_errors=True)
        raise


if __name__ == "__main__":
    main()
