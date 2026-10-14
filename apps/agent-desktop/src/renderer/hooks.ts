import { useEffect, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type {
  DesktopAction,
  DesktopBridge,
  DesktopState,
} from "@lyonbrown/agent-desktop/internal/shared/contract";
const KEY = ["desktop", "state"] as const;
export function useDesktopState(bridge: DesktopBridge) {
  const client = useQueryClient();
  const events = useRef(0);
  useEffect(
    () =>
      bridge.onState((state) => {
        ++events.current;
        client.setQueryData(KEY, state);
      }),
    [bridge, client],
  );
  return useQuery({
    queryKey: KEY,
    queryFn: async () => {
      const version = events.current;
      const state = await bridge.getState();
      return version === events.current ? state : (client.getQueryData<DesktopState>(KEY) ?? state);
    },
    retry: false,
  });
}
export function useDesktopActions(bridge: DesktopBridge) {
  const pending = useRef(false);
  const mutation = useMutation({
    mutationFn: async (action: DesktopAction | "reconnect") => {
      const result = action === "reconnect" ? await bridge.reconnect() : await bridge.act(action);
      if (!result.ok) throw new Error(result.error ?? "ACTION_REJECTED");
    },
    retry: false,
  });
  const execute = async (action: DesktopAction | "reconnect"): Promise<boolean> => {
    if (pending.current) return false;
    pending.current = true;
    try {
      await mutation.mutateAsync(action);
      return true;
    } catch {
      return false;
    } finally {
      pending.current = false;
    }
  };
  return { execute, pending: mutation.isPending, error: mutation.error?.message ?? null };
}
