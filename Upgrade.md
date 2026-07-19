import React, { useState, useMemo } from "react";
import { X, Trash2, Receipt } from "lucide-react";
import { betHistory } from "@/lib/sportsData";

const QUICK_STAKES = [500, 1000, 2000, 5000];

const statusColors = {
  won: "bg-green-500",
  lost: "bg-red-500",
  pending: "bg-sunset-orange",
};

const statusLabels = {
  won: "Won",
  lost: "Lost",
  pending: "Pending",
};

export default function BetSlip({ bets, onRemoveBet, onClearSlip }) {
  const [stake, setStake] = useState(1000);
  const [activeTab, setActiveTab] = useState("slip");

  const totalOdds = useMemo(() => {
    if (bets.length === 0) return 1;
    return bets.reduce((acc, bet) => acc * bet.odds, 1);
  }, [bets]);

  const potentialWin = totalOdds * stake;

  const formatCurrency = (num) =>
    num.toLocaleString("en-NG", { minimumFractionDigits: 2, maximumFractionDigits: 2 });

  const handleStakeInput = (e) => {
    const raw = e.target.value.replace(/[^0-9]/g, "");
    setStake(raw === "" ? 0 : parseInt(raw, 10));
  };

  return (
    <aside className="w-[280px] bg-canvas border-l border-light-pearl shrink-0 flex flex-col sticky top-0 h-[calc(100vh-100px)] overflow-hidden">
      {/* Header */}
      <div className="px-4 pt-4 pb-0 shrink-0">
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2">
            <h3 className="font-inter text-[15px] font-bold text-midnight">Bet Slip</h3>
            {bets.length > 0 && (
              <span className="bg-sunset-orange text-white font-inter text-[11px] font-bold min-w-[20px] h-5 px-1.5 rounded-full flex items-center justify-center">
                {bets.length}
              </span>
            )}
          </div>
          {bets.length > 0 && (
            <button
              onClick={onClearSlip}
              className="flex items-center gap-1 text-silver-ash hover:text-midnight transition-colors"
              title="Clear all"
            >
              <Trash2 className="w-3.5 h-3.5" />
              <span className="font-inter text-[12px]">Clear</span>
            </button>
          )}
        </div>

        {/* Tabs */}
        <div className="flex border-b border-light-pearl">
          {["slip", "history"].map((tab) => (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={`flex-1 flex items-center justify-center gap-1.5 py-2 font-inter text-[13px] font-medium transition-colors border-b-2 -mb-px ${
                activeTab === tab
                  ? "border-midnight text-midnight"
                  : "border-transparent text-silver-ash hover:text-dark-shale"
              }`}
            >
              {tab === "slip" ? "Selections" : (
                <>
                  <Receipt className="w-3.5 h-3.5" />
                  History
                </>
              )}
            </button>
          ))}
        </div>
      </div>

      {/* Slip Tab */}
      {activeTab === "slip" && (
        <div className="flex flex-col flex-1 overflow-hidden">
          {/* Selections list */}
          <div className="flex-1 overflow-y-auto px-4 py-3 space-y-2">
            {bets.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-12 gap-3">
                <div className="w-12 h-12 rounded-full bg-cloud-whisper flex items-center justify-center">
                  <Receipt className="w-5 h-5 text-silver-ash" />
                </div>
                <p className="font-inter text-[13px] text-silver-ash text-center">
                  Click any odds to<br />add selections
                </p>
              </div>
            ) : (
              bets.map((bet) => (
                <div key={bet.id} className="bg-cloud-whisper rounded-lg p-3 relative group border border-transparent hover:border-light-pearl transition-all">
                  <button
                    onClick={() => onRemoveBet(bet.id)}
                    className="absolute top-2 right-2 text-silver-ash hover:text-midnight opacity-0 group-hover:opacity-100 transition-opacity"
                  >
                    <X className="w-3.5 h-3.5" />
                  </button>
                  <div className="font-inter text-[11px] text-silver-ash mb-1 pr-5 truncate">{bet.match}</div>
                  <div className="flex items-center justify-between pr-4">
                    <span className="font-inter text-[13px] font-semibold text-midnight leading-tight">{bet.selection}</span>
                    <span className="font-inter text-[15px] font-bold text-sunset-orange ml-2 shrink-0">{bet.odds.toFixed(2)}</span>
                  </div>
                </div>
              ))
            )}
          </div>

          {/* Stake & Payout — always visible at bottom */}
          <div className="shrink-0 border-t border-light-pearl px-4 pt-3 pb-4 space-y-3 bg-canvas">
            {/* Stake input */}
            <div>
              <div className="flex items-center justify-between mb-1.5">
                <label className="font-inter text-[12px] font-semibold text-dark-shale uppercase tracking-wide">Stake</label>
                <span className="font-inter text-[12px] text-silver-ash">₦</span>
              </div>
              <input
                type="text"
                value={stake === 0 ? "" : stake.toLocaleString("en-NG")}
                onChange={handleStakeInput}
                placeholder="Enter amount"
                className="w-full border border-midnight/20 rounded-lg px-3 py-2 font-inter text-[15px] font-semibold text-midnight focus:outline-none focus:border-sunset-orange transition-colors bg-canvas text-right"
              />
            </div>

            {/* Quick stake buttons */}
            <div className="grid grid-cols-4 gap-1.5">
              {QUICK_STAKES.map((amount) => (
                <button
                  key={amount}
                  onClick={() => setStake(amount)}
                  className={`py-1 rounded font-inter text-[11px] font-medium border transition-all ${
                    stake === amount
                      ? "bg-midnight text-white border-midnight"
                      : "bg-cloud-whisper text-dark-shale border-light-pearl hover:border-silver-ash"
                  }`}
                >
                  {amount >= 1000 ? `${amount / 1000}K` : amount}
                </button>
              ))}
            </div>

            {/* Summary rows */}
            <div className="space-y-1.5 bg-cloud-whisper rounded-lg px-3 py-2.5">
              <div className="flex items-center justify-between">
                <span className="font-inter text-[12px] text-silver-ash">{bets.length} Selection{bets.length !== 1 ? "s" : ""}</span>
                <span className="font-inter text-[12px] font-medium text-dark-shale">Accumulator</span>
              </div>
              <div className="flex items-center justify-between">
                <span className="font-inter text-[12px] text-silver-ash">Total Odds</span>
                <span className="font-inter text-[13px] font-bold text-midnight">{bets.length > 0 ? totalOdds.toFixed(2) : "—"}</span>
              </div>
              <div className="h-px bg-light-pearl my-1" />
              <div className="flex items-center justify-between">
                <span className="font-inter text-[13px] font-semibold text-dark-shale">Potential Win</span>
                <span className="font-inter text-[15px] font-bold text-midnight">
                  {bets.length > 0 ? `₦ ${formatCurrency(potentialWin)}` : "—"}
                </span>
              </div>
            </div>

            <button
              disabled={bets.length === 0 || stake === 0}
              className="w-full bg-sunset-orange text-white font-inter text-[14px] font-bold py-3 rounded-[20px] hover:bg-sunset-orange/90 transition-colors active:scale-[0.98] disabled:opacity-40 disabled:cursor-not-allowed tracking-wide"
            >
              PLACE BET — ₦ {stake > 0 ? formatCurrency(stake) : "0.00"}
            </button>
          </div>
        </div>
      )}

      {/* History Tab */}
      {activeTab === "history" && (
        <div className="flex-1 overflow-y-auto px-4 py-3 space-y-2">
          {betHistory.map((item) => (
            <div key={item.id} className="bg-cloud-whisper rounded-lg px-3 py-2.5 flex items-center gap-3">
              <span className={`w-2 h-2 rounded-full shrink-0 ${statusColors[item.status]}`} />
              <div className="flex-1 min-w-0">
                <div className="font-inter text-[13px] font-medium text-midnight truncate">{item.match}</div>
                <div className="font-inter text-[11px] text-silver-ash">{statusLabels[item.status]}</div>
              </div>
              <div className="text-right shrink-0">
                <div className="font-inter text-[13px] font-bold text-midnight">{item.amount}</div>
              </div>
            </div>
          ))}
        </div>
      )}
    </aside>
  );
}


------------------------------


import React, { useRef, useState } from "react";
import { liveMatches as defaultLiveMatches } from "@/lib/sportsData";
import { ChevronLeft, ChevronRight } from "lucide-react";

export default function LiveMatches({ onOddsClick, selectedOdds, matches = defaultLiveMatches }) {
  const scrollRef = useRef(null);
  const [animatingId, setAnimatingId] = useState(null);

  const scroll = (dir) => {
    if (scrollRef.current) {
      scrollRef.current.scrollBy({ left: dir * 300, behavior: "smooth" });
    }
  };

  const handleOddsClick = (match, market, odds) => {
    const id = `live-${match.id}-${market}`;
    setAnimatingId(id);
    setTimeout(() => setAnimatingId(null), 200);
    onOddsClick({
      matchId: match.id,
      match: `${match.home} vs ${match.away}`,
      selection: `${market} (${market === "1" ? "Home" : market === "2" ? "Away" : "Draw"})`,
      market,
      odds,
    });
  };

  const isSelected = (matchId, market) => {
    return selectedOdds.some((o) => o.matchId === matchId && o.market === market);
  };

  return (
    <div className="mb-6">
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-2">
          <span className="w-2 h-2 rounded-full bg-red-500 live-pulse" />
          <h2 className="font-inter text-lg font-bold text-midnight">LIVE NOW</h2>
          <span className="bg-sunset-orange/10 text-sunset-orange font-inter text-xs font-semibold px-2.5 py-0.5 rounded-full">
            {matches.length} Live
          </span>
        </div>
        <div className="flex gap-1">
          <button onClick={() => scroll(-1)} className="p-1 rounded-full hover:bg-slate-mist transition-colors">
            <ChevronLeft className="w-4 h-4 text-silver-ash" />
          </button>
          <button onClick={() => scroll(1)} className="p-1 rounded-full hover:bg-slate-mist transition-colors">
            <ChevronRight className="w-4 h-4 text-silver-ash" />
          </button>
        </div>
      </div>

      <div ref={scrollRef} className="flex gap-3 overflow-x-auto hide-scrollbar pb-1">
        {matches.map((match) => (
          <div
            key={match.id}
            className="bg-slate-mist rounded-lg p-4 min-w-[272px] shrink-0 flex flex-col"
          >
            <div className="flex items-center gap-1.5 mb-2">
              <span className="w-1.5 h-1.5 rounded-full bg-red-500 live-pulse" />
              <span className="font-inter text-[12px] text-silver-ash">
                {match.league} � {match.minute}
              </span>
            </div>
            <div className="font-inter text-[15px] font-bold text-midnight mb-1">
              {match.home} vs {match.away}
            </div>
            <div className="font-inter text-2xl font-bold text-sunset-orange text-center my-2">
              {match.homeScore} � {match.awayScore}
            </div>
            <div className="flex gap-2 mt-auto">
              {Object.entries(match.odds).map(([market, odds]) => {
                const btnId = `live-${match.id}-${market}`;
                const sel = isSelected(match.id, market);
                return (
                  <button
                    key={market}
                    onClick={() => handleOddsClick(match, market, odds)}
                    className={`flex-1 py-2 rounded-lg border font-inter text-[13px] font-medium transition-all ${
                      sel
                        ? "bg-sunset-orange border-sunset-orange text-white"
                        : "border-midnight/20 text-midnight hover:bg-sunset-orange hover:border-sunset-orange hover:text-white"
                    } ${animatingId === btnId ? "odds-pop" : ""}`}
                  >
                    <div className="text-[10px] opacity-60">{market}</div>
                    <div>{odds.toFixed(2)}</div>
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}


=======================

import React from "react";
import { marketTabs as defaultMarketTabs } from "@/lib/sportsData";

export default function MarketTabs({ activeMarket, setActiveMarket, tabs = defaultMarketTabs }) {
  return (
    <div className="flex gap-2 overflow-x-auto hide-scrollbar pb-1 mb-4">
      {tabs.map((tab) => (
        <button
          key={tab}
          onClick={() => setActiveMarket(tab)}
          className={`shrink-0 px-4 py-[6px] rounded-[20px] font-inter text-[13px] font-medium transition-all border ${
            activeMarket === tab
              ? "bg-midnight text-white border-midnight"
              : "bg-cloud-whisper text-dark-shale border-light-pearl hover:border-silver-ash"
          }`}
        >
          {tab}
        </button>
      ))}
    </div>
  );
}


------------------------


import React, { useMemo, useState } from "react";
import {
  matchesByLeague as defaultMatchesByLeague,
  oddsColumns as defaultOddsColumns,
  marketColumnMap as defaultMarketColumnMap,
  oddsLabelMap as defaultOddsLabelMap,
} from "@/lib/sportsData";
import { ChevronDown, ChevronUp } from "lucide-react";

const CONTRACT_MARKET_NAMES = {
  0: "1X2",
  1: "O/U 2.5",
  2: "GG / NG",
};

const CONTRACT_OUTCOME_LABELS = {
  0: ["1", "X", "2"],
  1: ["O2.5", "U2.5"],
  2: ["GG", "NG"],
};

function formatContractOdd(value) {
  const n = Number(value) / 1_000_000
  return Number.isFinite(n) && n > 0 ? n.toFixed(2) : "�";
}

function isContractGroup(group) {
  return Boolean(group && Array.isArray(group.markets));
}

export default function OddsTable({
  activeMarket,
  onOddsClick,
  selectedOdds,
  leagues = defaultMatchesByLeague,
  columns = defaultOddsColumns,
  columnMap = defaultMarketColumnMap,
  labelMap = defaultOddsLabelMap,
  groups = [],
}) {
  const [animatingId, setAnimatingId] = useState(null);
  const [expandedMatch, setExpandedMatch] = useState(null);

  const visibleColumns = columnMap[activeMarket]
    ? columns.filter((c) => columnMap[activeMarket].includes(c.key))
    : columns;

  const isSelected = (matchId, market) => {
    return selectedOdds.some((o) => o.matchId === matchId && o.market === market);
  };

  const handleLegacyClick = (match, col) => {
    const id = `${match.id}-${col.key}`;
    setAnimatingId(id);
    setTimeout(() => setAnimatingId(null), 200);
    onOddsClick({
      matchId: match.id,
      match: `${match.home} vs ${match.away}`,
      selection: `${col.key} (${labelMap[col.key] || col.key})`,
      market: col.key,
      odds: match.odds[col.key],
    });
  };

  const handleContractClick = (group, market, outcomeIndex, outcomeLabel) => {
    const id = `${group.groupId}-${market.marketId}-${outcomeLabel}`;
    setAnimatingId(id);
    setTimeout(() => setAnimatingId(null), 200);
    onOddsClick({
      matchId: group.groupId,
      groupId: group.groupId,
      marketId: market.marketId,
      match: group.title || `Group ${group.groupId}`,
      selection: `${outcomeLabel} (${CONTRACT_MARKET_NAMES[market.marketType] || market.title || `Market ${market.groupMarketIndex + 1}`})`,
      market: `${market.marketId}:${outcomeLabel}`,
      odds: Number(market.currentOdds?.[outcomeIndex] || 0) / 1_000_000,
      marketTitle: market.title,
      marketType: market.marketType,
      outcomeLabel,
    });
  };

  const contractGroups = useMemo(() => groups.filter(isContractGroup), [groups]);
  const useContractView = contractGroups.length > 0;

  if (useContractView) {
    return (
      <div className="border border-light-pearl rounded-lg overflow-hidden">
        <div className="bg-slate-mist flex items-center sticky top-0 z-10">
          <div className="font-inter text-[12px] font-semibold text-silver-ash w-[80px] px-3 py-2.5 shrink-0">
            Time
          </div>
          <div className="font-inter text-[12px] font-semibold text-silver-ash flex-1 min-w-[200px] px-2 py-2.5">
            Match
          </div>
          <div className="font-inter text-[12px] font-semibold text-silver-ash w-[120px] px-2 py-2.5 text-right shrink-0">
            Status
          </div>
          <div className="w-[44px] shrink-0" />
        </div>

        {contractGroups.map((group) => {
          const markets = [...group.markets].sort((a, b) => (a.raw?.groupMarketIndex ?? a.index) - (b.raw?.groupMarketIndex ?? b.index))
          const isExpanded = expandedMatch === group.groupId

          return (
            <React.Fragment key={group.groupId}>
              <div className="flex items-center bg-canvas hover:bg-cloud-whisper border-b border-light-pearl transition-colors group">
                <div className="font-inter text-[12px] text-silver-ash w-[80px] px-3 py-2.5 shrink-0">
                  {group.eventStartTime ? new Date(group.eventStartTime * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '�'}
                </div>
                <div className="flex-1 min-w-[200px] px-2 py-2.5">
                  <div className="font-inter text-[14px] font-semibold text-midnight">{group.title || `Group ${group.groupId}`}</div>
                  <div className="font-inter text-[12px] text-silver-ash">
                    {markets.length} markets � {group.resultFinalized ? `Final ${group.homeScore}-${group.awayScore}` : `Exposure ${group.currentExposure ? 'live' : 'open'}`}
                  </div>
                </div>
                <div className="w-[120px] px-2 py-2.5 text-right shrink-0">
                  <div className="font-inter text-[12px] font-semibold text-midnight">{group.status || 'Live'}</div>
                  <div className="font-inter text-[11px] text-silver-ash">Group #{group.groupId}</div>
                </div>
                <div className="w-[44px] shrink-0 flex items-center justify-center">
                  <button
                    onClick={() => setExpandedMatch(isExpanded ? null : group.groupId)}
                    className="flex items-center gap-0.5 font-inter text-[12px] text-sunset-orange hover:underline"
                    aria-label={isExpanded ? 'Collapse match markets' : 'Expand match markets'}
                  >
                    {isExpanded ? <ChevronUp className="w-3 h-3" /> : <ChevronDown className="w-3 h-3" />}
                  </button>
                </div>
              </div>

              <div className={`border-b border-light-pearl ${isExpanded ? 'block' : 'block'}`}>
                <div className="bg-cloud-whisper px-4 py-3 space-y-3">
                  {markets.slice(0, 3).map((market) => {
                    const labels = CONTRACT_OUTCOME_LABELS[market.marketType] || Array.from({ length: market.numOutcomes || 0 }, (_, i) => `O${i + 1}`)
                    const marketName = CONTRACT_MARKET_NAMES[market.marketType] || market.title || `Market ${market.groupMarketIndex + 1}`

                    return (
                      <div key={market.marketId} className="grid grid-cols-[180px_1fr] gap-3 items-center rounded-lg border border-light-pearl bg-canvas px-3 py-2">
                        <div>
                          <div className="font-inter text-[13px] font-semibold text-midnight">{marketName}</div>
                          <div className="font-inter text-[11px] text-silver-ash">
                            {market.status} � {market.startTime ? new Date(market.startTime * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '�'}
                          </div>
                        </div>

                        <div className="flex flex-wrap gap-2 justify-end">
                          {labels.map((outcomeLabel, outcomeIndex) => {
                            const btnId = `${group.groupId}-${market.marketId}-${outcomeLabel}`;
                            const sel = isSelected(group.groupId, `${market.marketId}:${outcomeLabel}`)
                            return (
                              <button
                                key={outcomeLabel}
                                onClick={() => handleContractClick(group, market, outcomeIndex, outcomeLabel)}
                                className={`min-w-[86px] px-3 py-2 rounded-lg border font-inter text-[13px] transition-all ${
                                  sel
                                    ? "bg-sunset-orange border-sunset-orange text-white font-semibold"
                                    : "bg-cloud-whisper border-light-pearl text-midnight hover:border-sunset-orange hover:text-sunset-orange"
                                } ${animatingId === btnId ? "odds-pop" : ""}`}
                              >
                                <div className="text-[10px] opacity-60">{outcomeLabel}</div>
                                <div>{formatContractOdd(market.currentOdds?.[outcomeIndex])}</div>
                              </button>
                            )
                          })}
                        </div>
                      </div>
                    )
                  })}
                </div>
              </div>
            </React.Fragment>
          )
        })}
      </div>
    )
  }

  return (
    <div className="border border-light-pearl rounded-lg overflow-hidden">
      <div className="bg-slate-mist flex items-center sticky top-0 z-10">
        <div className="font-inter text-[12px] font-semibold text-silver-ash w-[60px] px-3 py-2.5 shrink-0">
          Time
        </div>
        <div className="font-inter text-[12px] font-semibold text-silver-ash flex-1 min-w-[140px] px-2 py-2.5">
          Match
        </div>
        {visibleColumns.map((col) => (
          <div
            key={col.key}
            className="font-inter text-[12px] font-semibold text-silver-ash w-[52px] text-center px-1 py-2.5 shrink-0"
          >
            {col.label}
          </div>
        ))}
        <div className="w-[44px] shrink-0" />
      </div>

      {leagues.map((league) => (
        <div key={league.league}>
          <div className="bg-cloud-whisper px-4 py-2 border-b border-light-pearl">
            <span className="font-inter text-[13px] font-bold text-dark-shale">{league.league}</span>
          </div>
          {league.matches.map((match) => (
            <React.Fragment key={match.id}>
              <div className="flex items-center bg-canvas hover:bg-cloud-whisper border-b border-light-pearl transition-colors group">
                <div className="font-inter text-[12px] text-silver-ash w-[60px] px-3 py-2.5 shrink-0">
                  {match.time}
                </div>
                <div className="flex-1 min-w-[140px] px-2 py-2.5">
                  <span className="font-inter text-[14px] font-semibold text-midnight">{match.home}</span>
                  <span className="font-inter text-[13px] text-silver-ash mx-1.5">vs</span>
                  <span className="font-inter text-[14px] text-dark-shale">{match.away}</span>
                </div>
                {visibleColumns.map((col) => {
                  const btnId = `${match.id}-${col.key}`;
                  const sel = isSelected(match.id, col.key);
                  return (
                    <div key={col.key} className="w-[52px] px-1 py-1.5 shrink-0 flex justify-center">
                      <button
                        onClick={() => handleLegacyClick(match, col)}
                        className={`w-[44px] py-1 rounded font-inter text-[13px] border transition-all ${
                          sel
                            ? "bg-sunset-orange border-sunset-orange text-white font-semibold"
                            : "bg-cloud-whisper border-light-pearl text-midnight hover:border-sunset-orange hover:text-sunset-orange"
                        } ${animatingId === btnId ? "odds-pop" : ""}`}
                      >
                        {match.odds[col.key]?.toFixed(2)}
                      </button>
                    </div>
                  );
                })}
                <div className="w-[44px] shrink-0 flex items-center justify-center">
                  <button
                    onClick={() => setExpandedMatch(expandedMatch === match.id ? null : match.id)}
                    className="flex items-center gap-0.5 font-inter text-[12px] text-sunset-orange hover:underline"
                  >
                    +{match.more}
                    {expandedMatch === match.id ? (
                      <ChevronUp className="w-3 h-3" />
                    ) : (
                      <ChevronDown className="w-3 h-3" />
                    )}
                  </button>
                </div>
              </div>
              {expandedMatch === match.id && (
                <div className="bg-cloud-whisper px-6 py-4 border-b border-light-pearl">
                  <div className="font-inter text-[12px] text-silver-ash mb-2">All Markets � {match.home} vs {match.away}</div>
                  <div className="flex flex-wrap gap-2">
                    {columns.map((col) => {
                      const sel = isSelected(match.id, col.key);
                      return (
                        <button
                          key={col.key}
                          onClick={() => handleLegacyClick(match, col)}
                          className={`px-3 py-1.5 rounded-lg font-inter text-[12px] border transition-all ${
                            sel
                              ? "bg-sunset-orange border-sunset-orange text-white"
                              : "bg-canvas border-light-pearl text-midnight hover:border-sunset-orange"
                          }`}
                        >
                          <span className="text-silver-ash mr-1">{col.label}</span>
                          <span className="font-semibold">{match.odds[col.key]?.toFixed(2)}</span>
                        </button>
                      );
                    })}
                  </div>
                </div>
              )}
            </React.Fragment>
          ))}
        </div>
      ))}
    </div>
  );
}


---------------------


import React from "react";
import { sportsCategories, popularLeagues } from "@/lib/sportsData";

export default function Sidebar({ activeSport, setActiveSport }) {
  return (
    <aside className="w-[220px] bg-canvas border-r border-light-pearl shrink-0 overflow-y-auto h-full hidden lg:block">
      <div className="py-4">
        <h3 className="font-inter text-[11px] font-semibold text-silver-ash uppercase tracking-wider px-5 mb-2">
          Sports
        </h3>
        {sportsCategories.map((sport) => {
          const isActive = activeSport === sport.name;
          return (
            <button
              key={sport.name}
              onClick={() => setActiveSport(sport.name)}
              className={`w-full flex items-center gap-3 px-5 py-[10px] font-inter text-[14px] transition-all text-left ${
                isActive
                  ? "border-l-[3px] border-l-sunset-orange bg-slate-mist text-midnight font-medium"
                  : "border-l-[3px] border-l-transparent text-midnight hover:bg-cloud-whisper"
              }`}
            >
              <span className="text-base">{sport.icon}</span>
              <span className="flex-1">{sport.name}</span>
              <span className="font-inter text-[12px] text-silver-ash">({sport.count})</span>
            </button>
          );
        })}
      </div>

      <div className="border-t border-light-pearl py-4">
        <h3 className="font-inter text-[11px] font-semibold text-silver-ash uppercase tracking-wider px-5 mb-2">
          Popular Leagues
        </h3>
        {popularLeagues.map((league) => (
          <button
            key={league.name}
            className="w-full flex items-center gap-3 px-5 py-[9px] font-inter text-[13px] text-dark-shale hover:bg-cloud-whisper hover:text-midnight transition-colors text-left"
          >
            <span className="text-base">{league.flag}</span>
            <span>{league.name}</span>
          </button>
        ))}
      </div>
    </aside>
  );
}


-----------------------



import React from "react";

const defaultStats = [
  { icon: "stats", label: "Total Markets Today", value: "4,521" },
  { icon: "live", label: "Live Events", value: "12" },
  { icon: "soon", label: "Starting Soon (1hr)", value: "34" },
  { icon: "odds", label: "Highest Odds Today", value: "245.00" },
];

export default function StatsBar({ stats = defaultStats }) {
  return (
    <div className="bg-cloud-whisper border-b border-light-pearl px-6 lg:px-10 py-2 flex items-center gap-6 overflow-x-auto hide-scrollbar">
      {stats.map((stat, i) => (
        <React.Fragment key={stat.label}>
          {i > 0 && <span className="text-light-pearl hidden sm:block">|</span>}
          <div className="flex items-center gap-1.5 shrink-0">
            <span className="text-sm">{stat.icon}</span>
            <span className="font-inter text-[13px] text-dark-shale">{stat.label}:</span>
            <span className="font-inter text-[13px] font-semibold text-midnight">{stat.value}</span>
          </div>
        </React.Fragment>
      ))}
    </div>
  );
}


-----------------------



import React, { useMemo, useState } from 'react'
import { Copy, LogOut, Wallet, ExternalLink, Loader2 } from 'lucide-react'
import { useMagicSession } from '@/hooks/useMagicSession'
import { cn } from '@/lib/utils'

const navLinks = ['Live', 'Pre-Match', 'Outrights', 'My Bets', 'Results']

export default function TopNav({ activeNav, setActiveNav }) {
  const { isLoggedIn, isLoading, address, shortAddress, balanceEth, connect, logout, showWallet, hasConfig } = useMagicSession()
  const [copyState, setCopyState] = useState('idle')

  const walletLabel = useMemo(() => {
    if (!hasConfig) return 'Magic not configured'
    if (isLoggedIn) return shortAddress || 'Wallet connected'
    return 'Connect wallet'
  }, [hasConfig, isLoggedIn, shortAddress])

  const handleCopy = async () => {
    if (!address) return
    try {
      await navigator.clipboard.writeText(address)
      setCopyState('copied')
      window.setTimeout(() => setCopyState('idle'), 1200)
    } catch {
      setCopyState('error')
      window.setTimeout(() => setCopyState('idle'), 1200)
    }
  }

  return (
    <header className="sticky top-0 z-50 bg-canvas border-b border-light-pearl h-[60px] flex items-center px-6 lg:px-10">
      <div className="flex items-center gap-2 mr-8 shrink-0">
        <span className="text-lg">TB</span>
        <span className="font-inter font-bold text-lg text-midnight tracking-tight">TradeBook</span>
      </div>

      <nav className="hidden md:flex items-center gap-1 flex-1 justify-center">
        {navLinks.map((link) => (
          <button
            key={link}
            onClick={() => setActiveNav(link)}
            className={cn(
              'font-inter text-[15px] px-4 py-[18px] relative transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-midnight/30 rounded-md',
              activeNav === link ? 'text-midnight font-semibold' : 'text-dark-shale hover:text-midnight'
            )}
          >
            {link}
            {activeNav === link && (
              <span className="absolute bottom-0 left-1/2 -translate-x-1/2 w-8 h-[2px] bg-midnight rounded-full" />
            )}
          </button>
        ))}
      </nav>

      <div className="flex items-center gap-3 ml-auto shrink-0">
        {isLoggedIn && (
          <div className="hidden lg:flex items-center gap-2 bg-cloud-whisper border border-light-pearl rounded-full px-3 py-1.5">
            <span className="w-2 h-2 rounded-full bg-emerald-500" />
            <span className="font-inter text-[12px] text-dark-shale">{balanceEth} ETH</span>
          </div>
        )}

        {isLoggedIn && address && (
          <div className="hidden xl:flex items-center gap-2 bg-slate-mist border border-light-pearl rounded-full px-3 py-1.5">
            <span className="font-inter text-[12px] font-medium text-midnight">{shortAddress}</span>
            <button
              onClick={handleCopy}
              className="text-silver-ash hover:text-midnight transition-colors"
              aria-label="Copy wallet address"
              title={copyState === 'copied' ? 'Copied' : 'Copy address'}
            >
              <Copy className="w-3.5 h-3.5" />
            </button>
          </div>
        )}

        <button
          onClick={isLoggedIn ? showWallet : connect}
          disabled={isLoading || !hasConfig}
          className="inline-flex items-center gap-2 font-inter text-sm font-medium text-midnight border border-midnight px-4 py-1.5 rounded-[20px] hover:bg-midnight hover:text-white transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
        >
          {isLoading ? <Loader2 className="w-4 h-4 animate-spin" /> : <Wallet className="w-4 h-4" />}
          <span>{isLoading ? 'Loading' : walletLabel}</span>
        </button>

        {isLoggedIn ? (
          <>
            <button
              onClick={showWallet}
              className="hidden sm:inline-flex items-center gap-2 font-inter text-sm font-medium text-midnight border border-light-pearl bg-cloud-whisper px-4 py-1.5 rounded-[20px] hover:border-midnight hover:bg-slate-mist transition-colors"
            >
              <ExternalLink className="w-4 h-4" />
              Wallet
            </button>
            <button
              onClick={logout}
              className="hidden sm:inline-flex items-center gap-2 font-inter text-sm font-medium text-dark-shale border border-light-pearl px-4 py-1.5 rounded-[20px] hover:text-midnight hover:border-midnight transition-colors"
            >
              <LogOut className="w-4 h-4" />
              Disconnect
            </button>
          </>
        ) : null}
      </div>
    </header>
  )
}



---------------


import React, { useState, useCallback } from "react";
import TopNav from "@/components/tradebook/TopNav";
import StatsBar from "@/components/tradebook/StatsBar";
import Sidebar from "@/components/tradebook/Sidebar";
import LiveMatches from "@/components/tradebook/LiveMatches";
import MarketTabs from "@/components/tradebook/MarketTabs";
import OddsTable from "@/components/tradebook/OddsTable";
import BetSlip from "@/components/tradebook/BetSlip";
import { initialBetSlip, marketTabs as defaultMarketTabs, liveMatches as defaultLiveMatches, matchesByLeague as defaultMatchesByLeague, oddsColumns as defaultOddsColumns, marketColumnMap as defaultMarketColumnMap, oddsLabelMap as defaultOddsLabelMap } from "@/lib/sportsData";
import { useContractDashboard } from "@/hooks/useContractDashboard";

export default function Dashboard() {
  const { uiData } = useContractDashboard();
  const dashboardData = uiData || {};

  const [activeNav, setActiveNav] = useState("Pre-Match");
  const [activeSport, setActiveSport] = useState("Football");
  const [activeMarket, setActiveMarket] = useState(defaultMarketTabs[0]);
  const [betSlip, setBetSlip] = useState(initialBetSlip);

  const selectedOdds = betSlip.map((b) => ({ matchId: b.matchId, market: b.market }));

  const handleOddsClick = useCallback((selection) => {
    setBetSlip((prev) => {
      const exists = prev.find(
        (b) => b.matchId === selection.matchId && b.market === selection.market
      );
      if (exists) {
        return prev.filter((b) => b.id !== exists.id);
      }
      return [
        ...prev,
        {
          id: `bet-${Date.now()}-${Math.random().toString(36).substr(2, 5)}`,
          ...selection,
        },
      ];
    });
  }, []);

  const handleRemoveBet = useCallback((id) => {
    setBetSlip((prev) => prev.filter((b) => b.id !== id));
  }, []);

  const handleClearSlip = useCallback(() => {
    setBetSlip([]);
  }, []);

  return (
    <div className="h-screen flex flex-col bg-canvas overflow-hidden font-inter">
      <TopNav activeNav={activeNav} setActiveNav={setActiveNav} />
      <StatsBar stats={dashboardData.stats} />

      <div className="flex flex-1 overflow-hidden">
        <Sidebar activeSport={activeSport} setActiveSport={setActiveSport} />

        <main className="flex-1 overflow-y-auto px-4 lg:px-6 py-4">
          <LiveMatches
            onOddsClick={handleOddsClick}
            selectedOdds={selectedOdds}
            matches={dashboardData.liveMatches || defaultLiveMatches}
          />
          <MarketTabs
            activeMarket={activeMarket}
            setActiveMarket={setActiveMarket}
            tabs={dashboardData.marketTabs || defaultMarketTabs}
          />
          <OddsTable
            activeMarket={activeMarket}
            onOddsClick={handleOddsClick}
            selectedOdds={selectedOdds}
            leagues={dashboardData.matchesByLeague || defaultMatchesByLeague}
            columns={dashboardData.oddsColumns || defaultOddsColumns}
            columnMap={dashboardData.marketColumnMap || defaultMarketColumnMap}
            labelMap={dashboardData.oddsLabelMap || defaultOddsLabelMap}
            groups={dashboardData.groups || []}
          />
        </main>

        <BetSlip bets={betSlip} onRemoveBet={handleRemoveBet} onClearSlip={handleClearSlip} />
      </div>
    </div>
  );
}


