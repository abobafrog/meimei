# Starter data

This folder contains 512 recognized Quick, Draw! sketches sampled from three
categories: 256 snowflakes, 128 snowmen, and 128 trees. The JSONL files retain
only category and vector strokes; participant IDs, timestamps, and country
codes were discarded.

Source: <https://github.com/googlecreativelab/quickdraw-dataset>  
License: CC BY 4.0 <https://creativecommons.org/licenses/by/4.0/>

Google says the drawings were individually moderated, but also warns that
inappropriate content may remain. These thematic examples are a small starter
set for safe drawings, not a reliable moderation dataset. They contain no
human-labeled rejected Frokla drawings, and model quality must be judged using
fresh, representative decisions from the site's workshop.

The ONNX starter model is in `../models/base-nsfw-384/`. It is an Apache-2.0
third-party NSFW classifier and only suggests a manual review; moderators make
the final decision.
