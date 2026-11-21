// What the microphone check's loudest moment (chunk RMS after the gain, 0..1) means for speech
// recognition. Pure, so it can be tested.

export function judgeLevel(peak: number): string {
  if (peak < 0.003) return '幾乎沒有聲音：麥克風可能被靜音、選錯，或 Windows 沒有允許桌面應用程式使用麥克風';
  if (peak < 0.03) return '聲音偏小：把增益調高一級再檢查，或把電腦移近講者／音箱';
  if (peak > 0.7) return '聲音偏大，可能破音：把增益調低一級';
  return '音量正常，可以開始';
}
