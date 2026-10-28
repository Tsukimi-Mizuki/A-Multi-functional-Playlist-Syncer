<script setup lang="ts">
import { useRouter } from "vue-router";
import { ArrowLeft } from "lucide-vue-next";
import { Button } from "@/components/ui";

const props = withDefaults(
  defineProps<{
    title: string;
    description?: string;
    back?: boolean;
    backTo?: string;
  }>(),
  { back: false, backTo: "/" },
);

const router = useRouter();

function goBack(): void {
  void router.push(props.backTo);
}
</script>

<template>
  <div class="flex flex-wrap items-start justify-between gap-3">
    <div class="flex min-w-0 items-start gap-3">
      <Button v-if="props.back" variant="ghost" size="icon-sm" class="mt-0.5 shrink-0" @click="goBack">
        <ArrowLeft class="size-4" />
      </Button>
      <div class="min-w-0">
        <h1 class="text-lg font-semibold tracking-wide text-foreground">{{ props.title }}</h1>
        <p v-if="props.description" class="mt-0.5 text-xs leading-relaxed text-muted-foreground">
          {{ props.description }}
        </p>
      </div>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <slot name="actions" />
    </div>
  </div>
</template>
