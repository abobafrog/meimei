#!/usr/bin/env python3
"""Train small local classifiers from explicit moderator decisions and export ONNX."""
import argparse
import hashlib
import json
import math
import os
import random
import shutil
import tempfile
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
import torch
from torch import nn

ROOT = Path(__file__).resolve().parent
BUCKETS = 4096
IMAGE_SIZE = 224


def text_features(value):
    chars = list("^" + value.lower() + "$")
    result = np.zeros(BUCKETS, dtype=np.float32)
    total = 0
    for size in range(2, 6):
        for offset in range(max(0, len(chars) - size + 1)):
            gram = "".join(chars[offset:offset + size]).encode("utf-8")
            hashed = 0xCBF29CE484222325
            for byte in gram:
                hashed ^= byte
                hashed = (hashed * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
            result[hashed % BUCKETS] += 1.0
            total += 1
    if total:
        result /= math.sqrt(total)
    return result


def drawing_image(row):
    # JSONL exports group strokes into complete drawings. Keep rendering close
    # to src/ai.rs so a model sees the whole composition instead of one line.
    rgb = np.empty((IMAGE_SIZE, IMAGE_SIZE, 3), dtype=np.uint8)
    rgb[:] = (16, 17, 20)
    strokes = row.get("strokes") or [row]
    for stroke in strokes:
        color = np.frombuffer(bytes.fromhex(stroke["color"].lstrip("#")), dtype=np.uint8)
        radius = max(stroke["width"] / 2 * IMAGE_SIZE / 4800, 1.0)
        paths = stroke.get("segments") or [stroke["points"]]
        for points in paths:
            if not points:
                continue
            segments = zip(points, points[1:]) if len(points) > 1 else [(points[0], points[0])]
            for start, end in segments:
                x0 = start[0] / 4800 * (IMAGE_SIZE - 1)
                y0 = start[1] / 2800 * (IMAGE_SIZE - 1)
                x1 = end[0] / 4800 * (IMAGE_SIZE - 1)
                y1 = end[1] / 2800 * (IMAGE_SIZE - 1)
                steps = max(math.ceil(math.hypot(x1 - x0, y1 - y0)), 1)
                for index in range(steps + 1):
                    fraction = index / steps
                    x = x0 + (x1 - x0) * fraction
                    y = y0 + (y1 - y0) * fraction
                    min_x = max(math.floor(x - radius), 0)
                    max_x = min(math.ceil(x + radius), IMAGE_SIZE - 1)
                    min_y = max(math.floor(y - radius), 0)
                    max_y = min(math.ceil(y + radius), IMAGE_SIZE - 1)
                    grid_y, grid_x = np.ogrid[min_y:max_y + 1, min_x:max_x + 1]
                    mask = (grid_x - x) ** 2 + (grid_y - y) ** 2 <= radius ** 2
                    rgb[min_y:max_y + 1, min_x:max_x + 1][mask] = color

    return rgb.astype(np.float32).transpose(2, 0, 1) / 255.0


class SmallConv(nn.Module):
    def __init__(self):
        super().__init__()
        self.layers = nn.Sequential(
            nn.Conv2d(3, 8, 3, padding=1), nn.ReLU(), nn.MaxPool2d(2),
            nn.Conv2d(8, 16, 3, padding=1), nn.ReLU(), nn.MaxPool2d(2),
            nn.Conv2d(16, 24, 3, padding=1), nn.ReLU(), nn.AdaptiveAvgPool2d((1, 1)),
            nn.Flatten(), nn.Linear(24, 1), nn.Sigmoid(),
        )

    def forward(self, image):
        return self.layers(image)


class TextHead(nn.Module):
    def __init__(self):
        super().__init__()
        self.layers = nn.Sequential(nn.Linear(BUCKETS, 1), nn.Sigmoid())

    def forward(self, features):
        return self.layers(features)


def load_rows(path):
    rows = []
    seen = {}
    for line_number, line in enumerate(Path(path).read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        row = json.loads(line)
        if row.get("kind") not in ("text", "drawing") or row.get("label") not in (0, 1):
            raise ValueError("line %d: expected kind=text|drawing and label=0|1" % line_number)
        kind = row["kind"]
        if kind == "text" and not isinstance(row.get("text"), str):
            raise ValueError("line %d: text missing" % line_number)
        if kind == "drawing":
            strokes = row.get("strokes") or [row]
            if not strokes or any(not isinstance(stroke.get("points"), list) or stroke.get("color") is None or stroke.get("width") is None for stroke in strokes):
                raise ValueError("line %d: drawing strokes missing points/color/width" % line_number)
        digest = hashlib.sha256(json.dumps(row, ensure_ascii=False, sort_keys=True).encode()).hexdigest()
        if digest in seen:
            if seen[digest] != row["label"]:
                raise ValueError("same sample has conflicting moderator labels")
            continue
        seen[digest] = row["label"]
        rows.append(row)
    return rows


def split_rows(rows, kind):
    selected = [row for row in rows if row["kind"] == kind]
    groups = {0: [], 1: []}
    for row in selected:
        groups[row["label"]].append(row)
    if min(len(groups[0]), len(groups[1])) < 20:
        raise ValueError("%s needs at least 20 distinct examples for each class (allowed/rejected)" % kind)
    rng = random.Random(177)
    training, validation = [], []
    for group in groups.values():
        rng.shuffle(group)
        cut = max(4, len(group) // 5)
        validation.extend(group[:cut])
        training.extend(group[cut:])
    return training, validation


def fit(model, x, y, epochs=80):
    torch.manual_seed(177)
    model.train()
    optimizer = torch.optim.AdamW(model.parameters(), lr=0.015, weight_decay=0.02)
    loss_fn = nn.BCELoss()
    for _ in range(epochs):
        optimizer.zero_grad(set_to_none=True)
        loss = loss_fn(model(x).squeeze(1), y)
        loss.backward()
        optimizer.step()
    model.eval()
    with torch.inference_mode():
        scores = model(x).squeeze(1)
        return float(((scores >= 0.8) == (y == 1)).float().mean().item())


def export(model, example, destination, input_name):
    torch.onnx.export(
        model.eval(), example, str(destination),
        input_names=[input_name], output_names=["score"],
        opset_version=17, dynamo=False,
    )
    import onnx
    onnx.checker.check_model(str(destination))
    import onnxruntime as ort
    session = ort.InferenceSession(str(destination), providers=["CPUExecutionProvider"])
    with torch.inference_mode():
        expected = model(example).cpu().numpy()
    actual = session.run(["score"], {input_name: example.cpu().numpy()})[0]
    np.testing.assert_allclose(actual, expected, rtol=1e-4, atol=1e-5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("export", help="JSONL created by `cargo run -- export-training FILE`")
    parser.add_argument("--output", default=str(ROOT / "models"), help="versioned ONNX output directory")
    args = parser.parse_args()

    torch.set_num_threads(1)
    torch.manual_seed(177)
    rows = load_rows(args.export)
    text_train, text_val = split_rows(rows, "text")
    image_train, image_val = split_rows(rows, "drawing")
    text_model = TextHead()
    text_x = torch.from_numpy(np.stack([text_features(row["text"]) for row in text_train]))
    text_y = torch.tensor([row["label"] for row in text_train], dtype=torch.float32)
    fit(text_model, text_x, text_y)

    image_model = SmallConv()
    image_x = torch.from_numpy(np.stack([drawing_image(row) for row in image_train]))
    image_y = torch.tensor([row["label"] for row in image_train], dtype=torch.float32)
    fit(image_model, image_x, image_y)

    text_scores = text_model(torch.from_numpy(np.stack([text_features(row["text"]) for row in text_val]))).squeeze(1).detach().numpy()
    image_scores = image_model(torch.from_numpy(np.stack([drawing_image(row) for row in image_val]))).squeeze(1).detach().numpy()
    text_accuracy = float(np.mean((text_scores >= .8) == np.array([r["label"] == 1 for r in text_val])))
    image_accuracy = float(np.mean((image_scores >= .8) == np.array([r["label"] == 1 for r in image_val])))

    version = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    version_dir = output / version
    version_dir.mkdir(mode=0o700)
    try:
        export(text_model, torch.zeros((1, BUCKETS), dtype=torch.float32), version_dir / "text.onnx", "features")
        export(image_model, torch.zeros((1, 3, IMAGE_SIZE, IMAGE_SIZE), dtype=torch.float32), version_dir / "drawing.onnx", "image")
        metadata = {
            "version": version,
            "created_utc": datetime.now(timezone.utc).isoformat(),
            "text_samples": len(text_train) + len(text_val),
            "drawing_samples": len(image_train) + len(image_val),
            "validation_accuracy_at_0_8": {"text": text_accuracy, "drawing": image_accuracy},
            "decision": "assist_review_only",
            "input": {"text": "4096 hashed UTF-8 character ngrams, lengths 2..5", "drawing": "NCHW float RGB, 224x224, values 0..1"},
        }
        (version_dir / "model.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
        pointer = output / "current.json"
        temporary = output / (".current-%s.tmp" % version)
        temporary.write_text(json.dumps({"directory": version}, indent=2), encoding="utf-8")
        os.replace(temporary, pointer)
        print(json.dumps(metadata, ensure_ascii=False, indent=2))
    except Exception:
        shutil.rmtree(version_dir, ignore_errors=True)
        raise


if __name__ == "__main__":
    main()
