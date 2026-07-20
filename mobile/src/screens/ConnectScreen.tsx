import { useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";

import { colors } from "@/components/theme";
import { useStereodrome } from "@/context/StereodromeContext";

export function ConnectScreen() {
  const stereodrome = useStereodrome();
  const [url, setUrl] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function connect() {
    if (stereodrome.manualOfflineEnabled) {
      setError("Offline mode is enabled");
      return;
    }

    setBusy(true);
    setError(null);
    try {
      await stereodrome.connect({ url, username, password });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <View style={styles.container}>
      <Text style={styles.title}>Subsonic Login</Text>
      <TextInput
        autoCapitalize="none"
        onChangeText={setUrl}
        placeholder="Server URL"
        style={styles.input}
        value={url}
      />
      <TextInput
        autoCapitalize="none"
        onChangeText={setUsername}
        placeholder="Username"
        style={styles.input}
        value={username}
      />
      <TextInput
        onChangeText={setPassword}
        placeholder="Password"
        secureTextEntry
        style={styles.input}
        value={password}
      />
      <Pressable
        disabled={busy || stereodrome.manualOfflineEnabled}
        onPress={() => {
          void connect();
        }}
        style={styles.button}
      >
        {busy ? (
          <ActivityIndicator color="#fff" />
        ) : (
          <Text style={styles.buttonText}>Connect</Text>
        )}
      </Pressable>
      <Text numberOfLines={2} style={styles.error}>
        {error ?? stereodrome.error ?? " "}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    justifyContent: "center",
    padding: 14,
  },
  title: {
    color: colors.text,
    fontSize: 17,
    fontWeight: "800",
    marginBottom: 10,
  },
  input: {
    backgroundColor: "#ffffff",
    borderColor: "#c9c9c1",
    borderRadius: 4,
    borderWidth: 1,
    color: colors.text,
    fontSize: 13,
    height: 34,
    marginBottom: 7,
    paddingHorizontal: 8,
  },
  button: {
    alignItems: "center",
    backgroundColor: colors.selected,
    borderRadius: 4,
    height: 34,
    justifyContent: "center",
    marginTop: 3,
  },
  buttonText: {
    color: "#fff",
    fontWeight: "800",
  },
  error: {
    color: "#b91c1c",
    fontSize: 11,
    marginTop: 7,
  },
});
