import { useEffect, useMemo, useRef, useState } from "react";
import {
  FlatList,
  Pressable,
  StyleSheet,
  Text,
  View,
  type ListRenderItemInfo,
} from "react-native";
import { ChevronRight } from "lucide-react-native";

import { useInputBus } from "@/context/InputContext";
import { colors } from "@/components/theme";

const rowHeight = 34;

export type SelectableOption = {
  label: string;
  sublabel?: string;
  onSelect(): void | Promise<void>;
  onLongSelect?(): void | Promise<void>;
};

export function SelectableList({
  options,
  empty = "Nothing here",
}: {
  options: SelectableOption[];
  empty?: string;
}) {
  const [activeIndex, setActiveIndex] = useState(0);
  const { subscribe } = useInputBus();
  const listRef = useRef<FlatList<SelectableOption>>(null);
  const rowLongPressed = useRef(false);
  const optionsSignature = useMemo(
    () =>
      options
        .map((option) => `${option.label}\u001f${option.sublabel ?? ""}`)
        .join("\u001e"),
    [options]
  );

  useEffect(() => {
    setActiveIndex(0);
  }, [optionsSignature]);

  useEffect(() => {
    if (options.length === 0) return;

    requestAnimationFrame(() => {
      if (activeIndex === 0) {
        listRef.current?.scrollToOffset({
          animated: false,
          offset: 0,
        });
        return;
      }

      listRef.current?.scrollToIndex({
        animated: true,
        index: activeIndex,
        viewOffset: 8,
        viewPosition: 0.5,
      });
    });
  }, [activeIndex, options.length]);

  useEffect(
    () =>
      subscribe((input) => {
        if (input === "scroll_forward") {
          setActiveIndex((index) => Math.min(options.length - 1, index + 1));
        }
        if (input === "scroll_backward") {
          setActiveIndex((index) => Math.max(0, index - 1));
        }
        if (input === "select") {
          void options[activeIndex]?.onSelect();
        }
        if (input === "select_long") {
          void options[activeIndex]?.onLongSelect?.();
        }
      }),
    [activeIndex, options, subscribe]
  );

  const data = useMemo(() => options, [options]);

  if (options.length === 0) {
    return (
      <View style={styles.empty}>
        <Text style={styles.emptyText}>{empty}</Text>
      </View>
    );
  }

  return (
    <FlatList
      ref={listRef}
      data={data}
      getItemLayout={(_, index) => ({
        length: rowHeight,
        offset: rowHeight * index,
        index,
      })}
      keyExtractor={(item, index) => `${item.label}-${index}`}
      onScrollToIndexFailed={(info) => {
        listRef.current?.scrollToOffset({
          animated: true,
          offset: Math.max(0, info.averageItemLength * info.index),
        });
      }}
      renderItem={({ item, index }: ListRenderItemInfo<SelectableOption>) => {
        const selected = index === activeIndex;
        return (
          <Pressable
            delayLongPress={450}
            onLongPress={() => {
              rowLongPressed.current = true;
              setActiveIndex(index);
              void item.onLongSelect?.();
            }}
            onPress={() => {
              if (rowLongPressed.current) {
                rowLongPressed.current = false;
                return;
              }
              setActiveIndex(index);
              void item.onSelect();
            }}
            style={[styles.row, selected && styles.selected]}
          >
            <View style={styles.labelGroup}>
              <Text
                numberOfLines={1}
                style={[styles.label, selected && styles.selectedText]}
              >
                {item.label}
              </Text>
              {item.sublabel ? (
                <Text
                  numberOfLines={1}
                  style={[styles.sublabel, selected && styles.selectedText]}
                >
                  {item.sublabel}
                </Text>
              ) : null}
            </View>
            <ChevronRight
              color={selected ? colors.selectedText : colors.muted}
              size={16}
            />
          </Pressable>
        );
      }}
    />
  );
}

const styles = StyleSheet.create({
  row: {
    height: rowHeight,
    alignItems: "center",
    flexDirection: "row",
    paddingHorizontal: 8,
  },
  selected: {
    backgroundColor: colors.selected,
  },
  labelGroup: {
    flex: 1,
  },
  label: {
    color: colors.text,
    fontSize: 15,
    fontWeight: "700",
  },
  sublabel: {
    color: colors.muted,
    fontSize: 11,
  },
  selectedText: {
    color: colors.selectedText,
  },
  empty: {
    alignItems: "center",
    flex: 1,
    justifyContent: "center",
  },
  emptyText: {
    color: colors.muted,
    fontWeight: "700",
  },
});
