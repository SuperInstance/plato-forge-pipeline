"""Forge pipeline orchestration."""
import time
from dataclasses import dataclass, field
from enum import Enum

class PipelineStage(Enum):
    LISTEN = "listen"
    BUFFER = "buffer"
    EMIT = "emit"
    COMPLETE = "complete"

@dataclass
class PipelineEvent:
    stage: PipelineStage
    data: dict
    timestamp: float = field(default_factory=time.time)
    success: bool = True
    error: str = ""

class ForgePipeline:
    def __init__(self):
        self._events: list[PipelineEvent] = []
        self._buffer: list[dict] = []
        self._emitted: list[dict] = []
        self.stage = PipelineStage.LISTEN
        self.total_processed = 0

    def listen(self, data: dict) -> PipelineEvent:
        self.stage = PipelineStage.LISTEN
        evt = PipelineEvent(stage=self.stage, data=data)
        self._events.append(evt)
        self._buffer.append(data)
        return evt

    def buffer_drain(self, max_items: int = 100, curriculum: bool = True) -> list[dict]:
        self.stage = PipelineStage.BUFFER
        items = self._buffer[:max_items]
        self._buffer = self._buffer[max_items:]
        if curriculum and len(items) > 1:
            items.sort(key=lambda x: -x.get("priority", 0.5))
        return items

    def emit(self, items: list[dict]) -> list[PipelineEvent]:
        self.stage = PipelineStage.EMIT
        events = []
        for item in items:
            evt = PipelineEvent(stage=self.stage, data=item)
            self._events.append(evt)
            self._emitted.append(item)
            self.total_processed += 1
            events.append(evt)
        return events

    def process(self, data: dict) -> PipelineEvent:
        self.listen(data)
        items = self.buffer_drain()
        if items:
            self.emit(items)
        self.stage = PipelineStage.COMPLETE
        return PipelineEvent(stage=self.stage, data=data, success=True)

    def flush(self):
        items = self.buffer_drain(max_items=len(self._buffer))
        if items:
            self.emit(items)

    @property
    def stats(self) -> dict:
        stages = {}
        for e in self._events:
            stages[e.stage.value] = stages.get(e.stage.value, 0) + 1
        return {"processed": self.total_processed, "buffered": len(self._buffer),
                "emitted": len(self._emitted), "events": stages}
