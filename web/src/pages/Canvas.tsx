import { useEffect, useMemo, useState } from 'react';
import {
  Activity,
  Blocks,
  Bot,
  Cable,
  CircleDot,
  Cpu,
  FolderKanban,
  Radar,
  Sparkles,
  Workflow,
} from 'lucide-react';
import { getCanvas } from '@/lib/api';
import { useSSE } from '@/hooks/useSSE';
import type { CanvasLane, CanvasSnapshot, SSEEvent } from '@/types/api';

function statusClasses(status: string): string {
  switch (status) {
    case 'active':
      return 'border-emerald-500/40 bg-emerald-500/10 text-emerald-200';
    case 'warn':
      return 'border-amber-500/40 bg-amber-500/10 text-amber-100';
    default:
      return 'border-slate-700 bg-slate-900/80 text-slate-200';
  }
}

function eventTone(type: string): string {
  switch (type) {
    case 'agent_start':
    case 'agent_end':
      return 'bg-cyan-500/15 text-cyan-200 border-cyan-400/30';
    case 'tool_call':
    case 'tool_call_start':
      return 'bg-fuchsia-500/15 text-fuchsia-200 border-fuchsia-400/30';
    case 'error':
      return 'bg-rose-500/15 text-rose-200 border-rose-400/30';
    default:
      return 'bg-slate-800 text-slate-300 border-slate-700';
  }
}

function eventLabel(event: SSEEvent): string {
  switch (event.type) {
    case 'agent_start':
      return `Agent started with ${event.provider ?? 'provider'} / ${event.model ?? 'model'}`;
    case 'agent_end':
      return `Agent completed in ${event.duration_ms ?? 0} ms`;
    case 'tool_call_start':
      return `Running ${event.tool ?? 'tool'}`;
    case 'tool_call':
      return `${event.tool ?? 'tool'} ${event.success ? 'succeeded' : 'finished'}`;
    case 'error':
      return `${event.component ?? 'component'}: ${event.message ?? 'error'}`;
    default:
      return event.type;
  }
}

function laneIcon(lane: CanvasLane) {
  switch (lane.id) {
    case 'inputs':
      return Radar;
    case 'core':
      return Bot;
    case 'automation':
      return Workflow;
    default:
      return CircleDot;
  }
}

export default function Canvas() {
  const [snapshot, setSnapshot] = useState<CanvasSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { events, status } = useSSE({ maxEvents: 40 });

  useEffect(() => {
    getCanvas()
      .then(setSnapshot)
      .catch((err: Error) => setError(err.message));
  }, []);

  const liveEvents = useMemo(() => [...events].reverse().slice(0, 8), [events]);

  if (error) {
    return (
      <div className="p-6">
        <div className="rounded-2xl border border-rose-700/50 bg-rose-950/40 p-4 text-rose-200">
          Failed to load canvas: {error}
        </div>
      </div>
    );
  }

  if (!snapshot) {
    return (
      <div className="flex h-64 items-center justify-center">
        <div className="h-8 w-8 animate-spin rounded-full border-2 border-cyan-400 border-t-transparent" />
      </div>
    );
  }

  return (
    <div className="min-h-full bg-[radial-gradient(circle_at_top,_rgba(34,211,238,0.18),_transparent_35%),linear-gradient(180deg,_rgba(15,23,42,1)_0%,_rgba(2,6,23,1)_100%)] p-6 text-white">
      <section className="rounded-[28px] border border-cyan-500/20 bg-slate-950/70 p-6 shadow-[0_0_0_1px_rgba(34,211,238,0.08),0_30px_80px_rgba(2,6,23,0.65)]">
        <div className="flex flex-col gap-6 xl:flex-row xl:items-end xl:justify-between">
          <div className="max-w-3xl">
            <div className="mb-3 inline-flex items-center gap-2 rounded-full border border-cyan-400/30 bg-cyan-400/10 px-3 py-1 text-xs uppercase tracking-[0.3em] text-cyan-200">
              <Sparkles className="h-3.5 w-3.5" />
              Live Visual Workspace
            </div>
            <h1 className="text-3xl font-semibold tracking-tight text-white md:text-4xl">
              {snapshot.title}
            </h1>
            <p className="mt-3 text-sm text-slate-300 md:text-base">
              {snapshot.subtitle}
            </p>
          </div>

          <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <div className="rounded-2xl border border-slate-800 bg-slate-900/80 p-4">
              <div className="flex items-center gap-2 text-xs uppercase tracking-[0.2em] text-slate-400">
                <Cable className="h-3.5 w-3.5" />
                Gateway
              </div>
              <p className="mt-3 text-lg font-semibold text-white">
                {snapshot.gateway.host}:{snapshot.gateway.port}
              </p>
              <p className="mt-1 text-sm text-slate-400">
                {snapshot.gateway.paired ? 'Paired dashboard session' : 'Waiting for pairing'}
              </p>
            </div>
            <div className="rounded-2xl border border-slate-800 bg-slate-900/80 p-4">
              <div className="flex items-center gap-2 text-xs uppercase tracking-[0.2em] text-slate-400">
                <FolderKanban className="h-3.5 w-3.5" />
                Workspace
              </div>
              <p className="mt-3 text-sm font-medium text-white">{snapshot.workspace_dir}</p>
              <p className="mt-1 text-sm text-slate-400">{snapshot.tools.total} registered tools</p>
            </div>
            <div className="rounded-2xl border border-slate-800 bg-slate-900/80 p-4">
              <div className="flex items-center gap-2 text-xs uppercase tracking-[0.2em] text-slate-400">
                <Activity className="h-3.5 w-3.5" />
                Event Stream
              </div>
              <p className="mt-3 text-lg font-semibold text-white capitalize">{status}</p>
              <p className="mt-1 text-sm text-slate-400">{events.length} events in session buffer</p>
            </div>
          </div>
        </div>
      </section>

      <section className="mt-6 grid gap-6 xl:grid-cols-[minmax(0,1.5fr)_420px]">
        <div className="rounded-[28px] border border-slate-800 bg-slate-950/80 p-5">
          <div className="mb-5 flex items-center gap-2 text-sm uppercase tracking-[0.28em] text-slate-400">
            <Blocks className="h-4 w-4 text-cyan-300" />
            Canvas Lanes
          </div>

          <div className="grid gap-4 lg:grid-cols-3">
            {snapshot.lanes.map((lane) => {
              const Icon = laneIcon(lane);
              return (
                <div
                  key={lane.id}
                  className="rounded-3xl border border-slate-800 bg-[linear-gradient(180deg,_rgba(15,23,42,0.92)_0%,_rgba(2,6,23,0.95)_100%)] p-4"
                >
                  <div className="mb-4 flex items-center gap-3">
                    <div className="rounded-2xl border border-cyan-400/25 bg-cyan-400/10 p-2 text-cyan-200">
                      <Icon className="h-4 w-4" />
                    </div>
                    <div>
                      <p className="text-xs uppercase tracking-[0.24em] text-slate-500">{lane.id}</p>
                      <h2 className="text-lg font-semibold text-white">{lane.label}</h2>
                    </div>
                  </div>

                  <div className="space-y-3">
                    {lane.nodes.map((node) => (
                      <article
                        key={node.id}
                        className={`rounded-2xl border p-4 transition-colors ${statusClasses(node.status)}`}
                      >
                        <div className="flex items-center justify-between gap-3">
                          <h3 className="text-sm font-semibold">{node.title}</h3>
                          <span className="rounded-full border border-current/20 px-2 py-0.5 text-[11px] uppercase tracking-[0.24em]">
                            {node.status}
                          </span>
                        </div>
                        <p className="mt-2 text-sm opacity-90">{node.detail}</p>
                      </article>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>
        </div>

        <div className="space-y-6">
          <section className="rounded-[28px] border border-slate-800 bg-slate-950/80 p-5">
            <div className="mb-4 flex items-center gap-2 text-sm uppercase tracking-[0.28em] text-slate-400">
              <Cpu className="h-4 w-4 text-cyan-300" />
              Runtime Summary
            </div>
            <div className="grid grid-cols-3 gap-3">
              <div className="rounded-2xl border border-emerald-500/20 bg-emerald-500/10 p-4">
                <p className="text-xs uppercase tracking-[0.2em] text-emerald-100/80">Active</p>
                <p className="mt-2 text-2xl font-semibold text-white">{snapshot.integrations.active}</p>
              </div>
              <div className="rounded-2xl border border-cyan-500/20 bg-cyan-500/10 p-4">
                <p className="text-xs uppercase tracking-[0.2em] text-cyan-100/80">Available</p>
                <p className="mt-2 text-2xl font-semibold text-white">{snapshot.integrations.available}</p>
              </div>
              <div className="rounded-2xl border border-slate-700 bg-slate-900 p-4">
                <p className="text-xs uppercase tracking-[0.2em] text-slate-400">Planned</p>
                <p className="mt-2 text-2xl font-semibold text-white">{snapshot.integrations.coming_soon}</p>
              </div>
            </div>

            <div className="mt-4 rounded-2xl border border-slate-800 bg-slate-900/80 p-4">
              <div className="flex items-center justify-between gap-4">
                <p className="text-sm font-medium text-white">Health Components</p>
                <span className="text-xs uppercase tracking-[0.24em] text-slate-400">
                  PID {snapshot.health.pid}
                </span>
              </div>
              <div className="mt-3 space-y-2">
                {Object.entries(snapshot.health.components).length === 0 ? (
                  <p className="text-sm text-slate-400">No health components have reported yet.</p>
                ) : (
                  Object.entries(snapshot.health.components).map(([name, component]) => (
                    <div
                      key={name}
                      className="flex items-center justify-between rounded-2xl border border-slate-800 bg-slate-950 px-3 py-2"
                    >
                      <span className="text-sm text-white">{name}</span>
                      <span className="text-xs uppercase tracking-[0.2em] text-slate-400">
                        {component.status}
                      </span>
                    </div>
                  ))
                )}
              </div>
            </div>
          </section>

          <section className="rounded-[28px] border border-slate-800 bg-slate-950/80 p-5">
            <div className="mb-4 flex items-center gap-2 text-sm uppercase tracking-[0.28em] text-slate-400">
              <Radar className="h-4 w-4 text-cyan-300" />
              Live Feed
            </div>
            <div className="space-y-3">
              {liveEvents.length === 0 ? (
                <p className="rounded-2xl border border-slate-800 bg-slate-900/70 p-4 text-sm text-slate-400">
                  Waiting for runtime activity. Start an agent run or a tool call to light up the canvas.
                </p>
              ) : (
                liveEvents.map((event, index) => (
                  <article
                    key={`${event.timestamp ?? 'event'}-${index}`}
                    className={`rounded-2xl border p-4 ${eventTone(event.type)}`}
                  >
                    <div className="flex items-center justify-between gap-3">
                      <p className="text-sm font-semibold uppercase tracking-[0.24em]">{event.type}</p>
                      <span className="text-xs opacity-70">{event.timestamp ?? 'live'}</span>
                    </div>
                    <p className="mt-2 text-sm">{eventLabel(event)}</p>
                  </article>
                ))
              )}
            </div>
          </section>
        </div>
      </section>
    </div>
  );
}
