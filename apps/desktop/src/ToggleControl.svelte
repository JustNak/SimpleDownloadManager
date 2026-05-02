<script lang="ts">
  interface Props {
    label: string;
    checked: boolean;
  }

  let { label, checked = $bindable(false) }: Props = $props();
</script>

<label class="flex items-center justify-between rounded border border-border bg-background px-3 py-2 text-sm">
  <span>{label}</span>
  <input type="checkbox" bind:checked />
</label>
