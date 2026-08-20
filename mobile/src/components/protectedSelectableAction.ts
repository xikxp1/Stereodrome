import { useCallback, useState } from "react";

import type { SelectableOption } from "@/components/SelectableList";
import { haptics } from "@/services/haptics";

export type ProtectedSelectableAction = {
  id: string;
  label: string;
  sublabel: string;
  confirmLabel: string;
  confirmSublabel: string;
  cancelLabel?: string;
  cancelSublabel?: string;
  disabled?: boolean;
  kind?: SelectableOption["kind"];
  onConfirm(): void | Promise<void>;
};

export function useProtectedSelectableAction(resetKey: string | number | null) {
  const [previousResetKey, setPreviousResetKey] = useState(resetKey);
  const [pendingActionId, setPendingActionId] = useState<string | null>(null);

  if (!Object.is(previousResetKey, resetKey)) {
    setPreviousResetKey(resetKey);
    setPendingActionId(null);
  }

  const visiblePendingActionId = Object.is(previousResetKey, resetKey)
    ? pendingActionId
    : null;

  const clearProtectedAction = useCallback(() => {
    setPendingActionId(null);
  }, []);

  const armProtectedAction = useCallback((id: string) => {
    setPendingActionId(id);
  }, []);

  const protectedActionRows = useCallback(
    (action: ProtectedSelectableAction): SelectableOption[] => {
      if (visiblePendingActionId !== action.id) {
        return [
          {
            kind: action.kind ?? "action",
            label: action.label,
            sublabel: action.sublabel,
            ...(action.disabled === undefined
              ? {}
              : { disabled: action.disabled }),
            onSelect: () => {
              setPendingActionId(action.id);
            },
          },
        ];
      }

      return [
        {
          kind: "action",
          label: action.cancelLabel ?? "Cancel",
          sublabel: action.cancelSublabel ?? "Keep current state",
          onSelect: clearProtectedAction,
        },
        {
          kind: "action",
          label: action.confirmLabel,
          sublabel: action.confirmSublabel,
          ...(action.disabled === undefined
            ? {}
            : { disabled: action.disabled }),
          wheelOnly: true,
          onSelect: async () => {
            haptics.warning();
            clearProtectedAction();
            await action.onConfirm();
          },
        },
      ];
    },
    [clearProtectedAction, visiblePendingActionId]
  );

  return {
    armProtectedAction,
    clearProtectedAction,
    pendingActionId: visiblePendingActionId,
    protectedActionRows,
  };
}
