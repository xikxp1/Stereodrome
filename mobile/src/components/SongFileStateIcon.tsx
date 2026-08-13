import { CircleCheck, Download, LoaderCircle } from "lucide-react-native";
import type { StyleProp, ViewStyle } from "react-native";

import { colors } from "@/components/theme";
import type { SongFileState } from "@/types/music";

export function SongFileStateIcon({
  selected = false,
  state,
  style,
}: {
  selected?: boolean;
  state: SongFileState;
  style?: StyleProp<ViewStyle>;
}) {
  const color = selected
    ? colors.selectedText
    : state === "downloaded"
      ? "#16803a"
      : state === "downloading"
        ? "#2563eb"
        : colors.muted;

  if (state === "downloaded") {
    return <CircleCheck color={color} size={13} style={style} />;
  }
  if (state === "downloading") {
    return <LoaderCircle color={color} size={13} style={style} />;
  }
  return <Download color={color} size={13} style={style} />;
}
