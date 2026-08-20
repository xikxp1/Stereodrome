import {
  type KeyboardTypeOptions,
  Modal,
  Pressable,
  Text,
  TextInput,
  View,
} from "react-native";

import { settingsScreenStyles as styles } from "@/screens/SettingsScreen.styles";
import { haptics } from "@/services/haptics";

type SettingsTextEditModalProps = {
  config: {
    keyboardType?: KeyboardTypeOptions;
    title: string;
  } | null;
  error: string | null;
  onCancel(): void;
  onChangeValue(value: string): void;
  onSubmit(): void;
  saving: boolean;
  value: string;
};

export function SettingsTextEditModal({
  config,
  error,
  onCancel,
  onChangeValue,
  onSubmit,
  saving,
  value,
}: SettingsTextEditModalProps) {
  function cancel(): void {
    haptics.selection();
    onCancel();
  }

  function submit(): void {
    haptics.selection();
    onSubmit();
  }

  return (
    <Modal
      animationType="fade"
      onRequestClose={cancel}
      transparent
      visible={config !== null}
    >
      <View style={styles.modalOverlay}>
        <View style={styles.modalCard}>
          <Text style={styles.modalTitle}>{config?.title}</Text>
          <TextInput
            autoFocus
            keyboardType={config?.keyboardType ?? "default"}
            onChangeText={onChangeValue}
            onFocus={haptics.selection}
            onSubmitEditing={submit}
            selectTextOnFocus
            style={styles.modalInput}
            value={value}
          />
          {error !== null && error.length > 0 ? (
            <Text numberOfLines={2} style={styles.modalError}>
              {error}
            </Text>
          ) : null}
          <View style={styles.modalActions}>
            <Pressable
              disabled={saving}
              onPress={cancel}
              style={styles.modalButton}
            >
              <Text style={styles.modalButtonText}>Cancel</Text>
            </Pressable>
            <Pressable
              disabled={saving}
              onPress={submit}
              style={[styles.modalButton, styles.modalPrimaryButton]}
            >
              <Text style={styles.modalPrimaryButtonText}>
                {saving ? "Saving..." : "Save"}
              </Text>
            </Pressable>
          </View>
        </View>
      </View>
    </Modal>
  );
}
