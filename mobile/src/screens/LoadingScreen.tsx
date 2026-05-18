import { ActivityIndicator, StyleSheet, Text, View } from "react-native";

import { colors } from "@/components/theme";

export function LoadingScreen() {
  return (
    <View style={styles.container}>
      <ActivityIndicator color={colors.selected} />
      <Text style={styles.label}>Restoring</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    alignItems: "center",
    flex: 1,
    gap: 10,
    justifyContent: "center",
  },
  label: {
    color: colors.muted,
    fontSize: 12,
    fontWeight: "700",
  },
});
