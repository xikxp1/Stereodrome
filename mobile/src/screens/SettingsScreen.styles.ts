import { StyleSheet } from "react-native";

import { colors } from "@/components/theme";

export const settingsScreenStyles = StyleSheet.create({
  container: {
    flex: 1,
  },
  modalActions: {
    flexDirection: "row",
    gap: 8,
    justifyContent: "flex-end",
  },
  modalButton: {
    borderColor: "#b9b9b2",
    borderRadius: 4,
    borderWidth: 1,
    paddingHorizontal: 10,
    paddingVertical: 7,
  },
  modalButtonText: {
    color: colors.text,
    fontSize: 12,
    fontWeight: "800",
  },
  modalCard: {
    backgroundColor: "#f7f7ef",
    borderColor: "#b9b9b2",
    borderRadius: 8,
    borderWidth: 1,
    padding: 12,
    width: "82%",
  },
  modalInput: {
    backgroundColor: "#fff",
    borderColor: "#c9c9c1",
    borderRadius: 4,
    borderWidth: 1,
    color: colors.text,
    fontSize: 15,
    fontWeight: "700",
    height: 36,
    marginBottom: 10,
    paddingHorizontal: 8,
  },
  modalError: {
    color: "#b3261e",
    fontSize: 11,
    fontWeight: "700",
    marginBottom: 8,
  },
  modalOverlay: {
    alignItems: "center",
    backgroundColor: "rgba(0, 0, 0, 0.38)",
    flex: 1,
    justifyContent: "center",
    padding: 14,
  },
  modalPrimaryButton: {
    backgroundColor: colors.selected,
    borderColor: colors.selected,
  },
  modalPrimaryButtonText: {
    color: colors.selectedText,
    fontSize: 12,
    fontWeight: "800",
  },
  modalTitle: {
    color: colors.text,
    fontSize: 15,
    fontWeight: "800",
    marginBottom: 8,
  },
});
