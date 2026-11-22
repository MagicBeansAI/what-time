"""The transformer tagger behind the what-time Rust backend.

Feature-row IDs in, per-token role logits and clause-boundary logits out.
The architecture is a small pre-LayerNorm encoder
(summed feature-row embeddings + learned positions, N blocks of 4-head
self-attention and tanh-GELU feed-forward, mean-pooled context into the head).
Every op is chosen so inference can be mirrored exactly in Rust f32: plain
LayerNorm, softmax attention, tanh-GELU, tanh hidden.
"""
from __future__ import annotations

import math

import torch
from torch import Tensor, nn
from torch.nn import functional as F

from features import FEATURE_ROWS

ROLE_CLASSES = 40


def gelu_tanh(value: Tensor) -> Tensor:
    return 0.5 * value * (1 + torch.tanh(math.sqrt(2 / math.pi) * (value + 0.044715 * value.pow(3))))


class TimeTransformer(nn.Module):
    def __init__(
        self,
        feature_rows: int = FEATURE_ROWS,
        d_model: int = 96,
        layers: int = 3,
        heads: int = 4,
        ffn: int = 384,
        max_positions: int = 128,
    ) -> None:
        super().__init__()
        if feature_rows != FEATURE_ROWS:
            raise ValueError(f"The featurizer emits {FEATURE_ROWS} feature rows")
        self.feature_rows = feature_rows
        self.d_model = d_model
        self.heads = heads
        self.d_head = d_model // heads
        self.embedding = nn.Parameter(torch.empty(feature_rows, d_model))
        self.position = nn.Parameter(torch.empty(max_positions, d_model))
        self.blocks = nn.ModuleList(
            [self._block(d_model, heads, ffn) for _ in range(layers)]
        )
        self.ln_f = nn.LayerNorm(d_model)
        self.global_weight = nn.Parameter(torch.empty(d_model, d_model))
        self.global_bias = nn.Parameter(torch.zeros(d_model))
        self.head_weight = nn.Parameter(torch.empty(d_model * 2, 64))
        self.head_bias = nn.Parameter(torch.zeros(64))
        self.output_weight = nn.Parameter(torch.empty(64, ROLE_CLASSES + 1))
        self.output_bias = nn.Parameter(torch.zeros(ROLE_CLASSES + 1))
        for parameter in self.parameters():
            if parameter.dim() > 1:
                nn.init.normal_(parameter, std=0.02)

    def _block(self, d_model: int, heads: int, ffn: int):
        parent = self

        class Block(nn.Module):
            def __init__(inner) -> None:
                super().__init__()
                inner.ln1 = nn.LayerNorm(d_model)
                inner.qkv = nn.Linear(d_model, 3 * d_model)
                inner.attn_out = nn.Linear(d_model, d_model)
                inner.ln2 = nn.LayerNorm(d_model)
                inner.ff1 = nn.Linear(d_model, ffn)
                inner.ff2 = nn.Linear(ffn, d_model)

            def forward(inner, x: Tensor, valid: Tensor) -> Tensor:
                h = inner.ln1(x)
                batch, time, _ = h.shape
                q, k, v = inner.qkv(h).split(h.shape[-1], dim=-1)
                reshape = (batch, time, parent.heads, parent.d_head)
                q = q.view(reshape).transpose(1, 2)
                k = k.view(reshape).transpose(1, 2)
                v = v.view(reshape).transpose(1, 2)
                scores = q @ k.transpose(-2, -1) / math.sqrt(parent.d_head)
                scores = scores.masked_fill(
                    ~valid[:, None, None, :], torch.finfo(scores.dtype).min
                )
                attended = torch.softmax(scores, dim=-1) @ v
                attended = attended.transpose(1, 2).reshape(batch, time, -1)
                x = x + inner.attn_out(attended)
                h = inner.ln2(x)
                x = x + inner.ff2(gelu_tanh(inner.ff1(h)))
                # Zeroed like the reference tagger: padding must not reach
                # the pooled context, and padded batches must equal unpadded
                # inference token for token.
                return x * valid.unsqueeze(-1)

        return Block()

    def forward(
        self, rows: Tensor, valid: Tensor, neighbors: Tensor | None = None
    ) -> tuple[Tensor, Tensor]:
        row_mask = (rows != self.feature_rows).unsqueeze(-1)
        embedded = F.embedding(rows.clamp_max(self.feature_rows - 1), self.embedding)
        x = (embedded * row_mask).sum(dim=2) * valid.unsqueeze(-1)
        time = x.shape[1]
        x = x + self.position[:time].unsqueeze(0)
        for block in self.blocks:
            x = block(x, valid)
        x = self.ln_f(x) * valid.unsqueeze(-1)
        pooled = x.sum(dim=1) / valid.sum(dim=1, keepdim=True).clamp_min(1)
        context = torch.sigmoid(pooled @ self.global_weight + self.global_bias) * pooled
        joined = torch.cat(
            (x, context.unsqueeze(1).expand(-1, time, -1)), dim=-1
        )
        hidden = torch.tanh(joined @ self.head_weight + self.head_bias)
        output = hidden @ self.output_weight + self.output_bias
        return output[..., :ROLE_CLASSES], output[..., ROLE_CLASSES]


INTENT_HIDDEN = 64


class IntentEncoder(nn.Module):
    """Sentence intent: does the text ask for something to be scheduled
    ("request") or only talk about time ("mention": past events, habits,
    facts, titles, idioms)?

    A small encoder of its own over the tagger's feature rows, so it reads
    the words (lexical rows included) rather than the tagger's token states,
    which carry little sentence-level meaning. Its final states are pooled
    three ways — mean (overall content), max (any cue token: "used to", था,
    "remind"), attention with one learned query (which tokens matter) —
    then a tanh layer and one logit; positive means request. Exported with
    an `intent.` prefix beside the tagger's tensors; the same ops as the
    tagger, so Rust mirrors it with the tagger's encoder pass.
    """

    def __init__(
        self,
        feature_rows: int = FEATURE_ROWS,
        d_model: int = 64,
        layers: int = 2,
        heads: int = 4,
        ffn: int = 128,
        max_positions: int = 128,
    ) -> None:
        super().__init__()
        self.feature_rows = feature_rows
        self.d_model = d_model
        self.heads = heads
        self.d_head = d_model // heads
        self.embedding = nn.Parameter(torch.empty(feature_rows, d_model))
        self.position = nn.Parameter(torch.empty(max_positions, d_model))
        self.blocks = nn.ModuleList(
            [TimeTransformer._block(self, d_model, heads, ffn) for _ in range(layers)]
        )
        self.ln_f = nn.LayerNorm(d_model)
        self.query = nn.Parameter(torch.empty(d_model))
        self.hidden_weight = nn.Parameter(torch.empty(d_model * 3, INTENT_HIDDEN))
        self.hidden_bias = nn.Parameter(torch.zeros(INTENT_HIDDEN))
        self.output_weight = nn.Parameter(torch.empty(INTENT_HIDDEN, 1))
        self.output_bias = nn.Parameter(torch.zeros(1))
        for parameter in self.parameters():
            if parameter.dim() > 1:
                nn.init.normal_(parameter, std=0.02)
        nn.init.normal_(self.query, std=0.02)

    def forward(self, rows: Tensor, valid: Tensor) -> Tensor:
        row_mask = (rows != self.feature_rows).unsqueeze(-1)
        embedded = F.embedding(rows.clamp_max(self.feature_rows - 1), self.embedding)
        x = (embedded * row_mask).sum(dim=2) * valid.unsqueeze(-1)
        x = x + self.position[: x.shape[1]].unsqueeze(0)
        for block in self.blocks:
            x = block(x, valid)
        x = self.ln_f(x) * valid.unsqueeze(-1)
        mean = x.sum(dim=1) / valid.sum(dim=1, keepdim=True).clamp_min(1)
        maximum = x.masked_fill(~valid.unsqueeze(-1), torch.finfo(x.dtype).min).max(dim=1).values
        scores = (x @ self.query) / math.sqrt(self.d_model)
        scores = scores.masked_fill(~valid, torch.finfo(scores.dtype).min)
        attended = (torch.softmax(scores, dim=1).unsqueeze(-1) * x).sum(dim=1)
        features = torch.cat((mean, maximum, attended), dim=-1)
        hidden = torch.tanh(features @ self.hidden_weight + self.hidden_bias)
        return (hidden @ self.output_weight + self.output_bias)[:, 0]
