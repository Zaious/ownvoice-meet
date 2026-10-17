// Simplified Chinese never reaches the screen. The caption prompt asks for Traditional Chinese as
// used in Taiwan, yet a model now and then answers a whole line in Simplified (measured: 1 line in
// 10 on long lines with Claude Haiku 5.5). Such a line goes through OpenCC's s2twp conversion
// (Simplified to Taiwan Traditional, with Taiwan phrasing). Traditional text is never converted:
// s2twp would turn 干擾 into 幹擾 and 台灣 into 臺灣. A line counts as Simplified when it holds a
// character that only Simplified Chinese uses.

use std::sync::OnceLock;

use ferrous_opencc::config::BuiltinConfig;
use ferrous_opencc::OpenCC;

fn converter() -> Option<&'static OpenCC> {
    static CONV: OnceLock<Option<OpenCC>> = OnceLock::new();
    CONV.get_or_init(|| OpenCC::from_config(BuiltinConfig::S2twp).ok()).as_ref()
}

/// Common characters that exist only in Simplified Chinese (each has a different Traditional form,
/// and none is itself a Traditional character), so one of them marks a line as Simplified.
const SIMPLIFIED_ONLY: &str = "这们说为个会时对没过还现问题发经实样关见进开动学长给让认话应该机点从两车东间头边门马无变当岁网视频讲读写际复谈坏块组赖规务击类专业乐买卖气电书钱钟铁银错锁种丢严丰临举义乌习乡亚产亲仅众优伟传伤伦体佣侠侣侦侧侨俭债倾偿储儿兑兰兴养兽内册军农决况冻净凉减凤凭划则刚创删别剧劝办劳势勋区医华协单卢卫却厂厅历压厌县参双叙号叹吓吗吨听启吴呐员呜咏响哑唤啰啸喷团园围图圆圣场坚坛坝坟坠垄垦垫埘壮声壳处备夸夹夺奋奖妇妈娱娲娴婴宁宝宠审宪宫宽宾寻导将尔尘尝层属岂岗岛岭峡币师帅帐带帮帧帼广庄庆库庙庞废异弃张弥弯弹强归录彦彻径忆忧怀态怜总恋恳恶悬惊惯愤愿戏战户执扩扫扬扰抚抛抢护报担拟拥择挂挡挣挤挥损换据掳掷揽搀搁携摄摆摇摊撑撵擞攒敌数斋旧旷昙显晋晒晓晕暂术杀杂权条来杨极构枪枫柜标栈栋栏树桥档梦检棱椭横欢欧歼毁毕毙汇汉汤沟沥沦沧沪泪泷泸泽洁洒浅浆浇浊测济浏浑浓涂涛涝涡润涧涨涩渊渐渔渗温湾湿溃溅滚满滤滥滨滩潆灭灯灵灾灿炉炼烁烂热焕爱爷牵犹狈狮独狭猎猪献环玛玮珐琐琼瑶瓒画畅畴疗疟疮疯痒瘫癣皑盏盐监盖盘睁矫矿码砖础硕确碍礼祸祯离秃秆积称稳穷窃窍窑窜窝竖竞笔笼筛筝签简箩篮篱粮紧纠红纤约级纪纫纬纯纱纲纳纵纷纸纹纺线练细织终绍结绕绘络绝绞统绢绣继绩绪续绰绳维绵综绿缀缓编缘缠缩缴罗罚罢羡翘耸联聪肃肠肤肾肿胀胁胆胶脉脏脑脚脱脸舰舱艰艳节芜苇苍苏茎荐荚荡荣药莱莲获莹萤营萧萨蓝蔼虏虑虚虽蚀蚁蚂蛮蜗蝇补衬袄袜装裤观览觉触誉计订讥讨训议讯记讳讶许论讼设访证评识诈诉诊词译试诗诚诞询详语误诱请诸诺课谁调谅谊谋谍谎谐谓谜谢谣谦谨谱贝贞负贡财责贤败账货质贩贪贫贬购贮贯贱贴贵贷贸费贺贼资赋赌赏赐赔赚赛赞赠赵赶趋跃践踪轨轩转轮软轰轴轻载较辅辆辈辉辑输辖辩辫辽达迁迈运远违连迟选逊递逻遗邓邮邻郑酝酱酿释钉针钓钙钢钥钦钩钮钳铃铅铜铭铲铸铺链销锅锋锡锣锤锦键锯锻镇镜闪闭闯闲闷闸闹闻阀阁阅队阳阴阵阶陆陈陕险随隐隶难雏雾霉静鞑韦韧韩页顶项顺须顽顾顿颁预领颗颜额风飘飞饥饭饮饰饱饲饼饿馆馈驱驳驶驻驾骂骄骆验骑骗骤鱼鲁鲜鸟鸡鸣鸭鸽鹅鹰麦黄齐齿龙龟";

/// The text in Taiwan Traditional Chinese: converted only when it holds Simplified characters.
pub fn to_taiwan(text: &str) -> String {
    if !text.chars().any(|c| SIMPLIFIED_ONLY.contains(c)) {
        return text.to_string();
    }
    converter().map_or_else(|| text.to_string(), |c| c.convert(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplified_lines_become_taiwan_traditional() {
        assert_eq!(to_taiwan("我不太常这样做，但就像我说的，这个视频里没有别的可讲的了"), "我不太常這樣做，但就像我說的，這個影片裡沒有別的可講的了");
    }

    #[test]
    fn traditional_text_is_left_alone() {
        for t in [
            "我們來念一些補丁修正。修正了化石復育研究在「捕捉一隻野生寶可夢」目標上崩潰的問題。",
            "乾淨的頭髮、干擾、著作、發現、皇后、只有一隻、臺灣與台灣、裡面的麵包、後面。",
            "可以請你分享畫面，帶我們看一下時程嗎？",
            "山谷、豆腐、表格、首頁、骨架、面對、里長、准許、岳父、范先生。",
        ] {
            assert_eq!(to_taiwan(t), t);
        }
    }
}
