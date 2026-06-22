import { useCallback, useEffect, useState } from "react";

import type { SelectableOption } from "@/components/SelectableList";

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
  const [pendingActionId, setPendingActionId] = useState<string | null>(null);

  useEffect(() => {
    setPendingActionId(null);
  }, [resetKey]);

  const clearProtectedAction = useCallback(() => {
    setPendingActionId(null);
  }, []);

  const armProtectedAction = useCallback((id: string) => {
    setPendingActionId(id);
  }, []);

  const protectedActionRows = useCallback(
    (action: ProtectedSelectableAction): SelectableOption[] => {
      if (pendingActionId !== action.id) {
        return [
          {
            kind: action.kind ?? "action",
            label: action.label,
            sublabel: action.sublabel,
            disabled: action.disabled,
            onSelect: () => setPendingActionId(action.id),
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
          disabled: action.disabled,
          wheelOnly: true,
          onSelect: async () => {
            clearProtectedAction();
            await action.onConfirm();
          },
        },
      ];
    },
    [clearProtectedAction, pendingActionId]
  );

  return {
    armProtectedAction,
    clearProtectedAction,
    pendingActionId,
    protectedActionRows,
  };
}
